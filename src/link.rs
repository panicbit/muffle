use color_eyre::eyre::{Context, ContextCompat, Result, ensure};
use pipewire::keys::{LINK_INPUT_NODE, LINK_INPUT_PORT, LINK_OUTPUT_NODE, LINK_OUTPUT_PORT};
use pipewire::registry::GlobalObject;
use pipewire::spa::utils::dict::DictRef;
use pipewire::types::ObjectType;

#[derive(Copy, Clone, Debug)]
pub struct Link {
    id: u32,
    output_node: u32,
    output_port: u32,
    input_node: u32,
    input_port: u32,
}

impl Link {
    pub fn from_object(object: &GlobalObject<impl AsRef<DictRef>>) -> Result<Self> {
        ensure!(object.type_ == ObjectType::Link);

        let dict = object
            .props
            .as_ref()
            .context("missing link properties")?
            .as_ref();

        let get_u32 = |key| {
            dict.get(key)
                .with_context(|| format!("property `{key}` not found"))?
                .parse::<u32>()
                .with_context(|| format!("failed to parse property `{key}` as u32"))
        };

        Ok(Self {
            id: object.id,
            output_node: get_u32(&LINK_OUTPUT_NODE)?,
            output_port: get_u32(&LINK_OUTPUT_PORT)?,
            input_node: get_u32(&LINK_INPUT_NODE)?,
            input_port: get_u32(&LINK_INPUT_PORT)?,
        })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn output_node(&self) -> u32 {
        self.output_node
    }

    pub fn output_port(&self) -> u32 {
        self.output_port
    }

    pub fn input_node(&self) -> u32 {
        self.input_node
    }

    pub fn input_port(&self) -> u32 {
        self.input_port
    }

    pub fn contains_port(&self, id: u32) -> bool {
        id == self.output_port() || id == self.input_port()
    }
}
