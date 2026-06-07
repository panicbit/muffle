use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use color_eyre::eyre::{Context, Result};
use pipewire::context::ContextRc;
use pipewire::keys;
use pipewire::main_loop::MainLoopRc;
use pipewire::properties::PropertiesBox;
use pipewire::registry::{self, GlobalObject, RegistryRc};
use pipewire::spa::utils::dict::DictRef;
use pipewire::types::ObjectType;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::link::Link;

mod config;
mod filter;
mod link;
mod logging;
mod parsing;

const CONFIG_PATH: &str = "muffle.toml";

fn main() -> Result<()> {
    color_eyre::install()?;
    logging::init()?;
    let (config, config_rx) = Config::watch(CONFIG_PATH)?;
    let mainloop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let app = App::new(config, registry);
    let _attached_config_rx = config_rx.attach(mainloop.loop_(), move |config| {
        app.borrow_mut().on_config(config);
    });

    mainloop.run();

    Ok(())
}

struct App {
    config: Config,
    registry_listener: Option<registry::Listener>,
    registry: RegistryRc,
    objects: HashMap<u32, Rc<GlobalObject<PropertiesBox>>>,
    destroyed_links: Vec<Link>,
}

impl App {
    fn new(config: Config, registry: RegistryRc) -> Rc<RefCell<Self>> {
        let this = Rc::new(RefCell::new(Self {
            config,
            registry_listener: None,
            registry,
            objects: HashMap::new(),
            destroyed_links: Vec::new(),
        }));

        let listener = this
            .borrow()
            .registry
            .add_listener_local()
            .global({
                let this = this.clone();
                move |global| this.borrow_mut().on_global(global)
            })
            .global_remove({
                let this = this.clone();
                move |id| this.borrow_mut().on_global_remove(id)
            })
            .register();

        this.borrow_mut().registry_listener = Some(listener);

        this
    }

    fn on_config(&mut self, config: Config) {
        self.config = config;
        info!("📝 New config applied.");
    }

    fn on_global(&mut self, object: &GlobalObject<&DictRef>) {
        let object = Rc::new(object.to_owned());

        self.objects.insert(object.id, object.clone());

        let result = match object.type_ {
            ObjectType::Link => self.on_link(&object),
            _ => return,
        };

        if let Err(err) = result {
            error!("failed to handle new {:?} object: {err:?}", object.type_);
        }
    }

    fn on_global_remove(&mut self, id: u32) {
        let Some(object) = self.objects.remove(&id) else {
            error!("Tried to remove global <{id}>, but it did not exist");
            return;
        };

        let result = match object.type_ {
            ObjectType::Port => self.on_port_remove(&object),
            _ => return,
        };

        if let Err(err) = result {
            error!(
                "failed to handle {:?} object removal: {err:?}",
                object.type_
            );
        }
    }

    fn on_link(&mut self, object: &GlobalObject<PropertiesBox>) -> Result<()> {
        let link = Link::from_object(object).context("failed to parse object as link")?;

        let Some(output_node) = self.resolve_object(link.output_node()) else {
            warn!("Link without output node: {object:#?}");
            return Ok(());
        };
        let Some(input_node) = self.resolve_object(link.input_node()) else {
            warn!("Link without input node: {object:#?}");
            return Ok(());
        };

        let output_name = self.resolve_label(output_node);
        let input_name = self.resolve_label(input_node);

        if output_name.is_empty() {
            warn!("output node has no label: {output_node:#?}")
        }
        if input_name.is_empty() {
            warn!("input node has no label: {input_node:#?}")
        }

        let link_is_allowed = self.link_is_allowed(output_name, input_name);
        let icon = if link_is_allowed { '✅' } else { '❌' };

        info!("{icon} {output_name} -> {input_name}");

        if !link_is_allowed {
            self.destroy_link(link);
        }

        Ok(())
    }

    fn resolve_object(&self, id: u32) -> Option<&Rc<GlobalObject<PropertiesBox>>> {
        let Some(node) = self.objects.get(&id) else {
            warn!("Missing object with id `{id}`!");
            return None;
        };

        Some(node)
    }

    fn resolve_label<'a>(&self, object: &'a GlobalObject<PropertiesBox>) -> &'a str {
        object
            .props
            .as_ref()
            .and_then(|props| {
                props
                    .get(&keys::APP_NAME)
                    .or_else(|| props.get(&keys::APP_ID))
                    .or_else(|| props.get(&keys::NODE_DESCRIPTION))
                    .or_else(|| props.get(&keys::NODE_NAME))
            })
            .unwrap_or_default()
    }

    fn link_is_allowed(&self, output_name: &str, input_name: &str) -> bool {
        let context = &filter::Context {
            output_name,
            input_name,
        };

        for filter in &self.config.unlink {
            if filter.eval(context) {
                return false;
            }
        }

        true
    }

    fn destroy_link(&mut self, link: Link) {
        if self.config.log_only {
            warn!("(log_only = true; link not removed)");
            return;
        }

        if let Err(err) = self.registry.destroy_global(link.id()).into_result() {
            error!("Failed to remove link: {}", err);
            return;
        }

        debug!("destroyed_links += {link:?}");
        self.destroyed_links.push(link);
    }

    fn on_port_remove(&mut self, object: &GlobalObject<PropertiesBox>) -> Result<()> {
        self.destroyed_links.retain(|link| {
            let retain = !link.contains_port(object.id);

            if !retain {
                debug!("forgetting {link:?}")
            }

            retain
        });

        debug!("num retained links: {}", self.destroyed_links.len());

        Ok(())
    }
}
