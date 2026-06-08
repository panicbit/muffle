use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use color_eyre::eyre::{Context, Result};
use pipewire::context::ContextRc;
use pipewire::core::{self, CoreRc, PW_ID_CORE};
use pipewire::keys::{
    self, LINK_INPUT_NODE, LINK_INPUT_PORT, LINK_OUTPUT_NODE, LINK_OUTPUT_PORT, OBJECT_LINGER,
};
use pipewire::main_loop::MainLoopRc;
use pipewire::properties::{PropertiesBox, properties};
use pipewire::registry::{self, GlobalObject, RegistryRc};
use pipewire::spa::utils::dict::DictRef;
use pipewire::spa::utils::result::AsyncSeq;
use pipewire::types::ObjectType;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::link::Link;

mod config;
mod filter;
mod link;
mod logging;
mod parsing;
mod signal;

const CONFIG_PATH: &str = "muffle.toml";

fn main() -> Result<()> {
    color_eyre::install()?;
    logging::init()?;
    let (config, config_rx) = Config::watch(CONFIG_PATH)?;
    let mainloop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let app = App::new(config, mainloop.clone(), core, registry);

    let _attached_shutdown_rx = signal::shutdown().attach(mainloop.loop_(), {
        let app = app.clone();

        move |_| {
            let mut app = app.borrow_mut();

            if app.shutdown_seq.is_none() {
                app.on_shutdown();
            }
        }
    });

    let _attached_config_rx = config_rx.attach(mainloop.loop_(), move |config| {
        app.borrow_mut().on_config(config);
    });

    mainloop.run();

    Ok(())
}

struct App {
    config: Config,
    mainloop: MainLoopRc,
    core: CoreRc,
    core_listener: Option<core::Listener>,
    shutdown_seq: Option<AsyncSeq>,
    registry: RegistryRc,
    registry_listener: Option<registry::Listener>,
    objects: HashMap<u32, Rc<GlobalObject<PropertiesBox>>>,
    destroyed_links: Vec<Link>,
}

impl App {
    fn new(
        config: Config,
        mainloop: MainLoopRc,
        core: CoreRc,
        registry: RegistryRc,
    ) -> Rc<RefCell<Self>> {
        let this = Rc::new(RefCell::new(Self {
            config,
            mainloop,
            core,
            core_listener: None,
            shutdown_seq: None,
            registry,
            registry_listener: None,
            objects: HashMap::new(),
            destroyed_links: Vec::new(),
        }));

        let done_listener = this
            .borrow()
            .core
            .add_listener_local()
            .done({
                let this = this.clone();

                move |id, seq| {
                    let this = this.borrow();

                    if id == PW_ID_CORE && Some(seq) == this.shutdown_seq {
                        info!("Exiting.");
                        this.mainloop.quit();
                    }
                }
            })
            .register();

        this.borrow_mut().core_listener = Some(done_listener);

        let registry_listener = this
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

        this.borrow_mut().registry_listener = Some(registry_listener);

        this
    }

    fn on_config(&mut self, config: Config) {
        if self.shutdown_seq.is_some() {
            return;
        }

        self.config = config;
        info!("📝 New config applied.");

        self.recheck();
    }

    fn on_global(&mut self, object: &GlobalObject<&DictRef>) {
        if self.shutdown_seq.is_some() {
            return;
        }

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
        if self.shutdown_seq.is_some() {
            return;
        }

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

        let link_is_allowed = self.link_is_allowed_by_name(output_name, input_name);
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

    fn resolve_label_by_id(&self, object: u32) -> &str {
        self.resolve_object(object)
            .map(|object| self.resolve_label(object))
            .unwrap_or_default()
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

    fn link_is_allowed_by_id(&self, output_node: u32, input_node: u32) -> bool {
        let output_name = self.resolve_label_by_id(output_node);
        let input_name = self.resolve_label_by_id(input_node);

        if output_name.is_empty() {
            warn!("output node has no label: {output_node:#?}")
        }
        if input_name.is_empty() {
            warn!("input node has no label: {input_node:#?}")
        }

        self.link_is_allowed_by_name(output_name, input_name)
    }

    fn link_is_allowed_by_name(&self, output_name: &str, input_name: &str) -> bool {
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
        if self.shutdown_seq.is_some() {
            return Ok(());
        }

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

    fn recheck(&mut self) {
        self.recheck_destroyed_links();
        self.recheck_existing_links();
    }

    fn recheck_existing_links(&mut self) {
        let links = self
            .objects
            .values()
            .filter(|object| object.type_ == ObjectType::Link)
            .cloned()
            .collect::<Vec<_>>();

        for link in links {
            if let Err(err) = self.on_link(&link) {
                error!("Failed to recheck link: {err}");
            }
        }
    }

    fn recheck_destroyed_links(&mut self) {
        for i in (0..self.destroyed_links.len()).rev() {
            let link = &self.destroyed_links[i];
            let link_allowed = self.link_is_allowed_by_id(link.output_node(), link.input_node());

            if link_allowed || self.config.log_only {
                self.restore_link(link);
                self.destroyed_links.swap_remove(i);
            }
        }
    }

    fn restore_link(&self, link: &Link) {
        let result = self.core.create_object::<pipewire::link::Link>(
            "link-factory",
            &properties! {
                *LINK_OUTPUT_NODE => link.output_node().to_string(),
                *LINK_OUTPUT_PORT => link.output_port().to_string(),
                *LINK_INPUT_NODE => link.input_node().to_string(),
                *LINK_INPUT_PORT => link.input_port().to_string(),
                *OBJECT_LINGER => "true",
            },
        );

        let output_name = self.resolve_label_by_id(link.output_node());
        let input_name = self.resolve_label_by_id(link.input_node());

        match result {
            Ok(_) => info!("🩹 Restored: {output_name} -> {input_name}"),
            Err(_) => error!("💥 Failed to restore link"),
        }
    }

    fn on_shutdown(&mut self) {
        self.config.unlink = Vec::new();

        self.recheck();

        self.shutdown_seq = Some(self.core.sync(0).expect("failed to pw sync"));

        info!("👋 Bye!");
    }
}
