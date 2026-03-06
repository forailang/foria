use crate::ast::{DeclKind, Flow};
use crate::ir::Ir;
use crate::types::TypeRegistry;
use std::collections::HashMap;

/// Serializable program bundle for native/WASM distribution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgramBundle {
    pub entry_flow: Flow,
    pub entry_ir: Ir,
    pub type_registry: TypeRegistry,
    pub flow_registry: FlowRegistry,
    /// FFI registry metadata (serialized FfiRegistry). Present when the project
    /// declares `extern` blocks and `ffi` config in forai.json.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffi_registry: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FlowProgram {
    pub flow: crate::ast::Flow,
    pub ir: Ir,
    pub emit_name: Option<String>,
    pub fail_name: Option<String>,
    pub registry: TypeRegistry,
    pub kind: DeclKind,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct FlowRegistry {
    pub flows: HashMap<String, FlowProgram>,
    pub value_mocks: HashMap<String, serde_json::Value>,
}

impl FlowRegistry {
    pub fn new() -> Self {
        Self {
            flows: HashMap::new(),
            value_mocks: HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: String, program: FlowProgram) {
        self.flows.insert(name, program);
    }

    pub fn get(&self, name: &str) -> Option<&FlowProgram> {
        self.flows.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut FlowProgram> {
        self.flows.get_mut(name)
    }

    pub fn is_flow(&self, name: &str) -> bool {
        self.flows.contains_key(name)
    }

    pub fn get_value_mock(&self, name: &str) -> Option<&serde_json::Value> {
        self.value_mocks.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &FlowProgram)> {
        self.flows.iter()
    }

    pub fn with_value_mocks(&self, mocks: HashMap<String, serde_json::Value>) -> FlowRegistry {
        let mut new = self.clone();
        new.value_mocks = mocks;
        new
    }
}
