use canon::ir::CanonIR;
use canon::id::NodeId;
use std::collections::HashMap;

pub fn normalize_module_tree(ir: &CanonIR) -> HashMap<NodeId, String> {
    let mut map = HashMap::new();

    for node in ir.nodes.iter() {
        let path = match node.parent {
            Some(parent) => format!("{}/{}", parent.index(), node.id.index()),
            None => format!("{}", node.id.index()),
        };

        map.insert(node.id, path);
    }

    map
}
