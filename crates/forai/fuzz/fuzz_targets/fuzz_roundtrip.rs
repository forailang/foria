#![no_main]
use libfuzzer_sys::fuzz_target;

// If the parser succeeds, further stages should not panic.
// Parse → runtime flow / flow graph → lower.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let module = match forai::parser::parse_module_v1(s) {
            Ok(m) => m,
            Err(_) => return,
        };

        // Try runtime func parse (func/sink/source bodies)
        let _ = forai::parser::parse_runtime_func_from_module_v1(&module);

        // Try flow graph parse + lower for each flow decl
        for decl in &module.decls {
            if let forai_core::ast::TopDecl::Flow(flow_decl) = decl {
                if let Ok(graph) = forai::parser::parse_flow_graph_decl_v1(flow_decl) {
                    let _ = forai::parser::lower_flow_graph_to_flow(&graph);
                }
            }
        }
    }
});
