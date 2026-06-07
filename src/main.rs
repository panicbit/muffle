use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use color_eyre::eyre::Result;
use parking_lot::RwLock;
use pipewire::context::ContextRc;
use pipewire::keys;
use pipewire::main_loop::MainLoopRc;
use pipewire::properties::PropertiesBox;
use pipewire::registry::{self, GlobalObject, RegistryRc};
use pipewire::spa::utils::dict::DictRef;
use pipewire::types::ObjectType;
use tracing::{error, info, warn};

use crate::config::Config;

mod config;
mod filter;
mod logging;
mod parsing;

const CONFIG_PATH: &str = "muffle.toml";

fn main() -> Result<()> {
    color_eyre::install()?;
    logging::init()?;
    let config = Config::watch(CONFIG_PATH)?;
    let mainloop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let _app = App::new(config, registry);

    mainloop.run();

    Ok(())
}

struct App {
    config: Arc<RwLock<Config>>,
    registry_listener: Option<registry::Listener>,
    registry: RegistryRc,
    objects: HashMap<u32, Rc<GlobalObject<PropertiesBox>>>,
}

impl App {
    fn new(config: Arc<RwLock<Config>>, registry: RegistryRc) -> Rc<RefCell<Self>> {
        let this = Rc::new(RefCell::new(Self {
            config,
            registry_listener: None,
            registry,
            objects: HashMap::new(),
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

    fn on_global(&mut self, object: &GlobalObject<&DictRef>) {
        let object = Rc::new(object.to_owned());

        self.objects.insert(object.id, object.clone());

        if object.type_ == ObjectType::Link {
            self.on_link(&object);
        }
    }

    fn on_link(&mut self, object: &GlobalObject<PropertiesBox>) {
        let config = self.config.read();

        let Some(props) = &object.props else {
            warn!("Link without props");
            return;
        };

        let Some(output_node) = self.resolve_object(props, &keys::LINK_OUTPUT_NODE) else {
            warn!("Link without output node: {object:#?}");
            return;
        };
        let Some(input_node) = self.resolve_object(props, &keys::LINK_INPUT_NODE) else {
            warn!("Link without input node: {object:#?}");
            return;
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
            if config.log_only {
                warn!("(log_only = true; link not removed)")
            } else {
                self.registry.destroy_global(object.id);
            }
        }
    }

    fn resolve_object(
        &self,
        props: impl AsRef<DictRef>,
        key: &str,
    ) -> Option<&Rc<GlobalObject<PropertiesBox>>> {
        let props = props.as_ref();
        let Some(object_id) = props
            .get(key)
            .and_then(|node_id| node_id.parse::<u32>().ok())
        else {
            warn!("Missing `{key}`!");
            return None;
        };

        let Some(node) = self.objects.get(&object_id) else {
            warn!("Missing object with id `{object_id}`!");
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

    fn on_global_remove(&mut self, id: u32) {
        if self.objects.remove(&id).is_none() {
            error!("Tried to remove global <{id}>, but it did not exist")
        }
    }

    fn link_is_allowed(&self, output_name: &str, input_name: &str) -> bool {
        let config = self.config.read();
        let context = &filter::Context {
            output_name,
            input_name,
        };

        for filter in &config.unlink {
            if filter.eval(context) {
                return false;
            }
        }

        true
    }
}
