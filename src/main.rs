use std::cell::RefCell;
use std::collections::{HashMap, hash_map};
use std::rc::Rc;

use color_eyre::eyre::{Context, Result};
use pipewire::context::ContextRc;
use pipewire::main_loop::MainLoopRc;
use pipewire::properties::PropertiesBox;
use pipewire::registry::{self, GlobalObject, RegistryRc};
use pipewire::spa::utils::dict::DictRef;
use pipewire::types::ObjectType;
use regex::Regex;

use crate::config::{Config, Rule};

mod config;

fn main() -> Result<()> {
    let mainloop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let _app = App::new(registry);

    mainloop.run();

    Ok(())
}

struct App {
    registry_listener: Option<registry::Listener>,
    registry: RegistryRc,
    objects: HashMap<u32, Rc<GlobalObject<PropertiesBox>>>,
    regex_cache: RefCell<HashMap<String, Rc<Regex>>>,
}

impl App {
    fn new(registry: RegistryRc) -> Rc<RefCell<Self>> {
        let this = Rc::new(RefCell::new(Self {
            registry_listener: None,
            registry,
            objects: HashMap::new(),
            regex_cache: RefCell::new(HashMap::new()),
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
        let Some(props) = &object.props else {
            eprintln!("Link without props");
            return;
        };

        let Some(output_node) = self.resolve_object(props, "link.output.node") else {
            eprintln!("Link without output node: {object:#?}");
            return;
        };
        let Some(input_node) = self.resolve_object(props, "link.input.node") else {
            eprintln!("Link without input node: {object:#?}");
            return;
        };

        let output_name = self.resolve_label(output_node);
        let input_name = self.resolve_label(input_node);

        if output_name.is_empty() {
            println!("output node has no app name: {output_node:#?}")
        }
        if input_name.is_empty() {
            println!("input node has no app name: {input_node:#?}")
        }

        // let output_is_game = output_name == "ForbiddenSolitaire.exe";
        // let input_is_discord = input_name == "discord_capture";

        // let link_is_allowed = !input_is_discord || output_is_game;
        let link_is_allowed = self.link_is_allowed(output_name, input_name);
        let icon = if link_is_allowed { '✅' } else { '❌' };

        println!("{icon} {output_name} -> {input_name}");
        if !link_is_allowed {
            self.registry.destroy_global(object.id);
            println!("Removed link");
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
            eprintln!("Missing `{key}`!");
            return None;
        };

        let Some(node) = self.objects.get(&object_id) else {
            eprintln!("Missing object with id `{object_id}`!");
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
                    .get("application.name")
                    .or_else(|| props.get("node.description"))
                    .or_else(|| props.get("node.name"))
            })
            .unwrap_or_default()
    }

    fn on_global_remove(&mut self, id: u32) {
        if self.objects.remove(&id).is_none() {
            eprintln!("Tried to remove global <{id}>, but it did not exist")
        }
    }

    fn link_is_allowed(&self, output_name: &str, input_name: &str) -> bool {
        let config = match Config::load("muffle.toml") {
            Ok(config) => config,
            Err(err) => {
                eprintln!("Failed to load config: {err:?}");
                return true;
            }
        };

        for rule in config.rules {
            let input_pattern = match self.get_regex(rule.input_pattern) {
                Ok(pattern) => pattern,
                Err(err) => {
                    eprintln!("Invalid input_pattern: {err:?}");
                    return true;
                }
            };

            if !input_pattern.is_match(input_name) {
                continue;
            }

            let output_allow_pattern = match self.get_regex(rule.output_allow_pattern) {
                Ok(pattern) => pattern,
                Err(err) => {
                    eprintln!("Invalid output_allow_pattern: {err:?}");
                    return true;
                }
            };

            if !output_allow_pattern.is_match(output_name) {
                return false;
            }
        }

        true
    }

    fn get_regex(&self, pattern: String) -> Result<Rc<Regex>> {
        let mut regex_cache = self.regex_cache.borrow_mut();

        match regex_cache.entry(pattern) {
            hash_map::Entry::Occupied(entry) => Ok(entry.get().clone()),
            hash_map::Entry::Vacant(entry) => {
                let regex = Regex::new(entry.key()).context("invalid pattern")?;
                let regex = Rc::new(regex);

                entry.insert(regex.clone());

                Ok(regex)
            }
        }
    }
}
