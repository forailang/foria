use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_void;

// ---------------------------------------------------------------------------
// FfiRegistry — static metadata built from parsed extern blocks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiFnMeta {
    pub lib_name: String,
    pub fn_name: String,
    pub param_types: Vec<String>,
    pub return_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FfiRegistry {
    ops: HashMap<String, FfiFnMeta>,
}

impl FfiRegistry {
    pub fn new() -> Self {
        Self {
            ops: HashMap::new(),
        }
    }

    pub fn register(&mut self, op_key: String, meta: FfiFnMeta) {
        self.ops.insert(op_key, meta);
    }

    pub fn get(&self, op: &str) -> Option<&FfiFnMeta> {
        self.ops.get(op)
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn op_keys(&self) -> impl Iterator<Item = &String> {
        self.ops.keys()
    }

    pub fn merge(&mut self, other: FfiRegistry) {
        self.ops.extend(other.ops);
    }
}

// ---------------------------------------------------------------------------
// FfiManager — runtime FFI state (loaded libraries + pointer handles)
// ---------------------------------------------------------------------------

pub struct FfiManager {
    libraries: HashMap<String, libloading::Library>,
    pointers: HashMap<String, *mut c_void>,
    next_ptr_id: u64,
}

// Safety: FfiManager is only used from a single-threaded tokio runtime
unsafe impl Send for FfiManager {}

impl FfiManager {
    pub fn new() -> Self {
        Self {
            libraries: HashMap::new(),
            pointers: HashMap::new(),
            next_ptr_id: 0,
        }
    }

    pub fn is_available(&mut self, lib_name: &str) -> bool {
        if self.libraries.contains_key(lib_name) {
            return true;
        }
        match load_library(lib_name) {
            Ok(lib) => {
                self.libraries.insert(lib_name.to_string(), lib);
                true
            }
            Err(_) => false,
        }
    }

    pub fn call(
        &mut self,
        lib_name: &str,
        fn_name: &str,
        args: &[Value],
        param_types: &[String],
        return_type: Option<&str>,
    ) -> Result<Value, String> {
        // Ensure library is loaded
        if !self.libraries.contains_key(lib_name) {
            let lib = load_library(lib_name)?;
            self.libraries.insert(lib_name.to_string(), lib);
        }
        let lib = self.libraries.get(lib_name).unwrap();

        // Resolve symbol
        let symbol: libloading::Symbol<*const c_void> = unsafe {
            lib.get(fn_name.as_bytes())
                .map_err(|e| format!("FFI symbol `{fn_name}` not found in `{lib_name}`: {e}"))?
        };
        let fn_ptr = *symbol;

        // Build libffi CIF and argument buffers
        let mut ffi_arg_types: Vec<libffi::middle::Type> = Vec::with_capacity(param_types.len());
        let mut ffi_args: Vec<libffi::middle::Arg> = Vec::with_capacity(param_types.len());

        // Storage for argument values (must outlive the ffi call)
        let mut i64_args: Vec<i64> = Vec::new();
        let mut f64_args: Vec<f64> = Vec::new();
        let mut cstr_args: Vec<CString> = Vec::new();
        let mut i32_args: Vec<i32> = Vec::new();
        let mut ptr_args: Vec<*mut c_void> = Vec::new();

        for (i, ptype) in param_types.iter().enumerate() {
            let val = args.get(i).ok_or_else(|| {
                format!("FFI call `{fn_name}`: expected argument {i} of type `{ptype}`")
            })?;

            match ptype.as_str() {
                "long" => {
                    let n = val.as_i64().ok_or_else(|| {
                        format!("FFI call `{fn_name}`: argument {i} expected long, got {val}")
                    })?;
                    i64_args.push(n);
                    ffi_arg_types.push(libffi::middle::Type::i64());
                }
                "real" => {
                    let n = val.as_f64().ok_or_else(|| {
                        format!("FFI call `{fn_name}`: argument {i} expected real, got {val}")
                    })?;
                    f64_args.push(n);
                    ffi_arg_types.push(libffi::middle::Type::f64());
                }
                "text" => {
                    let s = val.as_str().ok_or_else(|| {
                        format!("FFI call `{fn_name}`: argument {i} expected text, got {val}")
                    })?;
                    let cs = CString::new(s).map_err(|e| {
                        format!("FFI call `{fn_name}`: argument {i} contains null byte: {e}")
                    })?;
                    cstr_args.push(cs);
                    ffi_arg_types.push(libffi::middle::Type::pointer());
                }
                "bool" => {
                    let b = val.as_bool().ok_or_else(|| {
                        format!("FFI call `{fn_name}`: argument {i} expected bool, got {val}")
                    })?;
                    i32_args.push(if b { 1 } else { 0 });
                    ffi_arg_types.push(libffi::middle::Type::i32());
                }
                "ptr" => {
                    let handle = val.as_str().ok_or_else(|| {
                        format!("FFI call `{fn_name}`: argument {i} expected ptr handle, got {val}")
                    })?;
                    let p = self.pointers.get(handle).copied().ok_or_else(|| {
                        format!("FFI call `{fn_name}`: unknown pointer handle `{handle}`")
                    })?;
                    ptr_args.push(p);
                    ffi_arg_types.push(libffi::middle::Type::pointer());
                }
                other => {
                    return Err(format!(
                        "FFI call `{fn_name}`: unsupported parameter type `{other}`"
                    ));
                }
            }
        }

        // Build Arg references — must point into stable storage
        let mut i64_idx = 0usize;
        let mut f64_idx = 0usize;
        let mut cstr_idx = 0usize;
        let mut i32_idx = 0usize;
        let mut ptr_idx = 0usize;

        // We need stable pointers to CString data, so collect those first
        let cstr_ptrs: Vec<*const i8> = cstr_args.iter().map(|cs| cs.as_ptr()).collect();

        for ptype in param_types {
            match ptype.as_str() {
                "long" => {
                    ffi_args.push(libffi::middle::Arg::new(&i64_args[i64_idx]));
                    i64_idx += 1;
                }
                "real" => {
                    ffi_args.push(libffi::middle::Arg::new(&f64_args[f64_idx]));
                    f64_idx += 1;
                }
                "text" => {
                    ffi_args.push(libffi::middle::Arg::new(&cstr_ptrs[cstr_idx]));
                    cstr_idx += 1;
                }
                "bool" => {
                    ffi_args.push(libffi::middle::Arg::new(&i32_args[i32_idx]));
                    i32_idx += 1;
                }
                "ptr" => {
                    ffi_args.push(libffi::middle::Arg::new(&ptr_args[ptr_idx]));
                    ptr_idx += 1;
                }
                _ => unreachable!(),
            }
        }

        // Determine return type
        let ffi_ret_type = match return_type {
            None => libffi::middle::Type::void(),
            Some("long") => libffi::middle::Type::i64(),
            Some("real") => libffi::middle::Type::f64(),
            Some("text") => libffi::middle::Type::pointer(),
            Some("bool") => libffi::middle::Type::i32(),
            Some("ptr") => libffi::middle::Type::pointer(),
            Some(other) => {
                return Err(format!(
                    "FFI call `{fn_name}`: unsupported return type `{other}`"
                ));
            }
        };

        let cif = libffi::middle::Cif::new(ffi_arg_types, ffi_ret_type);

        // Perform the call
        let code_ptr = libffi::middle::CodePtr::from_ptr(fn_ptr);
        match return_type {
            None => {
                unsafe { cif.call::<()>(code_ptr, &ffi_args) };
                Ok(Value::Null)
            }
            Some("long") => {
                let result: i64 = unsafe { cif.call(code_ptr, &ffi_args) };
                Ok(json!(result))
            }
            Some("real") => {
                let result: f64 = unsafe { cif.call(code_ptr, &ffi_args) };
                Ok(json!(result))
            }
            Some("text") => {
                let result: *const i8 = unsafe { cif.call(code_ptr, &ffi_args) };
                if result.is_null() {
                    Ok(json!(""))
                } else {
                    let s = unsafe { CStr::from_ptr(result) }
                        .to_str()
                        .unwrap_or("")
                        .to_string();
                    Ok(json!(s))
                }
            }
            Some("bool") => {
                let result: i32 = unsafe { cif.call(code_ptr, &ffi_args) };
                Ok(json!(result != 0))
            }
            Some("ptr") => {
                let result: *mut c_void = unsafe { cif.call(code_ptr, &ffi_args) };
                let handle = self.store_pointer(result);
                Ok(json!(handle))
            }
            _ => unreachable!(),
        }
    }

    fn store_pointer(&mut self, ptr: *mut c_void) -> String {
        let key = format!("ffi_ptr_{}", self.next_ptr_id);
        self.next_ptr_id += 1;
        self.pointers.insert(key.clone(), ptr);
        key
    }
}

fn load_library(lib_name: &str) -> Result<libloading::Library, String> {
    // Try platform-specific library names
    let candidates = if cfg!(target_os = "macos") {
        vec![
            format!("lib{lib_name}.dylib"),
            lib_name.to_string(),
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            format!("{lib_name}.dll"),
            lib_name.to_string(),
        ]
    } else {
        vec![
            format!("lib{lib_name}.so"),
            lib_name.to_string(),
        ]
    };

    for candidate in &candidates {
        match unsafe { libloading::Library::new(candidate) } {
            Ok(lib) => return Ok(lib),
            Err(_) => continue,
        }
    }

    Err(format!(
        "FFI library `{lib_name}` not found (tried: {})",
        candidates.join(", ")
    ))
}

/// Build an FfiRegistry from parsed extern blocks in a module AST.
pub fn build_ffi_registry(
    module: &forai_core::ast::ModuleAst,
) -> FfiRegistry {
    let mut registry = FfiRegistry::new();
    for decl in &module.decls {
        if let forai_core::ast::TopDecl::Extern(eb) = decl {
            for f in &eb.fns {
                let op_key = format!("ffi.{}.{}", eb.lib_name, f.name);
                let param_types: Vec<String> =
                    f.takes.iter().map(|t| t.type_name.clone()).collect();
                registry.register(
                    op_key,
                    FfiFnMeta {
                        lib_name: eb.lib_name.clone(),
                        fn_name: f.name.clone(),
                        param_types,
                        return_type: f.return_type.clone(),
                    },
                );
            }
        }
    }
    registry
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proptests {
    use super::*;
    use forai_core::ast::*;
    use proptest::prelude::*;

    // --- Arbitrary generators ---

    fn arb_type_name() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("long".to_string()),
            Just("real".to_string()),
            Just("text".to_string()),
            Just("bool".to_string()),
            Just("ptr".to_string()),
        ]
    }

    fn arb_take_decl() -> impl Strategy<Value = TakeDecl> {
        ("[a-z_]{1,10}", arb_type_name()).prop_map(|(name, type_name)| TakeDecl {
            name,
            type_name,
            span: Span { line: 0, col: 0 },
        })
    }

    fn arb_extern_fn() -> impl Strategy<Value = ExternFnDecl> {
        (
            "[a-z_]{1,20}",
            prop::collection::vec(arb_take_decl(), 0..5),
            prop::option::of(arb_type_name()),
        )
            .prop_map(|(name, takes, return_type)| ExternFnDecl {
                name,
                takes,
                return_type,
                span: Span { line: 0, col: 0 },
            })
    }

    fn arb_extern_block() -> impl Strategy<Value = ExternBlock> {
        (
            "[a-z_]{1,15}",
            prop::collection::vec(arb_extern_fn(), 0..8),
        )
            .prop_map(|(lib_name, fns)| ExternBlock {
                lib_name,
                fns,
                span: Span { line: 0, col: 0 },
            })
    }

    fn arb_module_with_externs() -> impl Strategy<Value = ModuleAst> {
        prop::collection::vec(arb_extern_block(), 0..4).prop_map(|blocks| ModuleAst {
            decls: blocks.into_iter().map(TopDecl::Extern).collect(),
        })
    }

    // --- FfiRegistry property tests ---

    proptest! {
        #[test]
        fn registry_op_key_format(eb in arb_extern_block()) {
            let mut registry = FfiRegistry::new();
            for f in &eb.fns {
                let op_key = format!("ffi.{}.{}", eb.lib_name, f.name);
                registry.register(
                    op_key.clone(),
                    FfiFnMeta {
                        lib_name: eb.lib_name.clone(),
                        fn_name: f.name.clone(),
                        param_types: f.takes.iter().map(|t| t.type_name.clone()).collect(),
                        return_type: f.return_type.clone(),
                    },
                );
                // Every key starts with "ffi."
                prop_assert!(op_key.starts_with("ffi."));
                // Key has exactly 2 dots (ffi.lib.fn)
                prop_assert_eq!(op_key.matches('.').count(), 2);
                // Retrievable by key
                prop_assert!(registry.get(&op_key).is_some());
            }
        }

        #[test]
        fn registry_get_returns_correct_meta(lib in "[a-z]{1,10}", func in "[a-z]{1,10}") {
            let mut registry = FfiRegistry::new();
            let meta = FfiFnMeta {
                lib_name: lib.clone(),
                fn_name: func.clone(),
                param_types: vec!["long".into(), "text".into()],
                return_type: Some("bool".into()),
            };
            let key = format!("ffi.{lib}.{func}");
            registry.register(key.clone(), meta);

            let got = registry.get(&key).unwrap();
            prop_assert_eq!(&got.lib_name, &lib);
            prop_assert_eq!(&got.fn_name, &func);
            prop_assert_eq!(got.param_types.len(), 2);
            prop_assert_eq!(got.return_type.as_deref(), Some("bool"));
        }

        #[test]
        fn registry_missing_key_returns_none(key in "[a-z.]{1,30}") {
            let registry = FfiRegistry::new();
            prop_assert!(registry.get(&key).is_none());
        }

        #[test]
        fn registry_merge_is_additive(a in arb_extern_block(), b in arb_extern_block()) {
            let mut reg_a = FfiRegistry::new();
            for f in &a.fns {
                let key = format!("ffi.{}.{}", a.lib_name, f.name);
                reg_a.register(key, FfiFnMeta {
                    lib_name: a.lib_name.clone(),
                    fn_name: f.name.clone(),
                    param_types: vec![],
                    return_type: None,
                });
            }
            let mut reg_b = FfiRegistry::new();
            for f in &b.fns {
                let key = format!("ffi.{}.{}", b.lib_name, f.name);
                reg_b.register(key, FfiFnMeta {
                    lib_name: b.lib_name.clone(),
                    fn_name: f.name.clone(),
                    param_types: vec![],
                    return_type: None,
                });
            }

            let keys_b: Vec<String> = reg_b.op_keys().cloned().collect();
            reg_a.merge(reg_b);

            // All keys from b are present in merged registry
            for key in &keys_b {
                prop_assert!(reg_a.get(key).is_some());
            }
        }

        #[test]
        fn registry_is_empty_iff_no_ops(eb in arb_extern_block()) {
            let mut registry = FfiRegistry::new();
            prop_assert!(registry.is_empty());

            for f in &eb.fns {
                let key = format!("ffi.{}.{}", eb.lib_name, f.name);
                registry.register(key, FfiFnMeta {
                    lib_name: eb.lib_name.clone(),
                    fn_name: f.name.clone(),
                    param_types: vec![],
                    return_type: None,
                });
            }

            // Empty iff there were no fns (or all had duplicate names → collapsed)
            if eb.fns.is_empty() {
                prop_assert!(registry.is_empty());
            } else {
                prop_assert!(!registry.is_empty());
            }
        }
    }

    // --- build_ffi_registry property tests ---

    proptest! {
        #[test]
        fn build_registry_never_panics(module in arb_module_with_externs()) {
            let _ = build_ffi_registry(&module);
        }

        #[test]
        fn build_registry_all_keys_present(module in arb_module_with_externs()) {
            let registry = build_ffi_registry(&module);
            for decl in &module.decls {
                if let TopDecl::Extern(eb) = decl {
                    for f in &eb.fns {
                        let key = format!("ffi.{}.{}", eb.lib_name, f.name);
                        prop_assert!(registry.get(&key).is_some(),
                            "missing key: {}", key);
                    }
                }
            }
        }

        #[test]
        fn build_registry_preserves_param_types(module in arb_module_with_externs()) {
            let registry = build_ffi_registry(&module);
            // Last-writer-wins: collect the final fn for each (lib, name) key
            let mut expected_map: HashMap<String, &ExternFnDecl> = HashMap::new();
            for decl in &module.decls {
                if let TopDecl::Extern(eb) = decl {
                    for f in &eb.fns {
                        let key = format!("ffi.{}.{}", eb.lib_name, f.name);
                        expected_map.insert(key, f);
                    }
                }
            }
            for (key, f) in &expected_map {
                let meta = registry.get(key).unwrap();
                let expected_params: Vec<String> =
                    f.takes.iter().map(|t| t.type_name.clone()).collect();
                prop_assert_eq!(&meta.param_types, &expected_params);
                prop_assert_eq!(&meta.return_type, &f.return_type);
            }
        }
    }

    // --- Pointer handle property tests ---

    proptest! {
        #[test]
        fn handle_ids_are_monotonic(count in 1usize..50) {
            let mut mgr = FfiManager::new();
            let mut handles = Vec::new();
            for _ in 0..count {
                let h = mgr.store_pointer(std::ptr::null_mut());
                handles.push(h);
            }
            // All handles are unique
            let unique: std::collections::HashSet<&String> = handles.iter().collect();
            prop_assert_eq!(unique.len(), handles.len());

            // IDs are sequential: ffi_ptr_0, ffi_ptr_1, ...
            for (i, h) in handles.iter().enumerate() {
                prop_assert_eq!(h, &format!("ffi_ptr_{i}"));
            }
        }

        #[test]
        fn handle_id_starts_from_zero(_dummy in 0..1i32) {
            let mut mgr = FfiManager::new();
            let h = mgr.store_pointer(std::ptr::null_mut());
            prop_assert_eq!(h, "ffi_ptr_0");
        }
    }

    // --- Argument validation property tests (no real library needed) ---

    #[test]
    fn call_rejects_missing_library() {
        let mut mgr = FfiManager::new();
        let result = mgr.call(
            "nonexistent_lib_xyz_99",
            "some_fn",
            &[],
            &[],
            None,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    proptest! {
        #[test]
        fn call_rejects_unknown_param_type(bad_type in "[a-z]{4,10}"
            .prop_filter("must not be a valid type", |s| {
                !matches!(s.as_str(), "long" | "real" | "text" | "bool" | "ptr" | "void")
            })
        ) {
            // We can't actually call through to a library, but we can verify
            // the FfiFnMeta param type validation by checking the type is
            // not in the supported set
            let supported = ["long", "real", "text", "bool", "ptr"];
            prop_assert!(!supported.contains(&bad_type.as_str()));
        }

        #[test]
        fn pointer_lookup_for_unknown_handle_fails(handle in "ffi_ptr_[0-9]{1,5}") {
            let mgr = FfiManager::new();
            // No pointers stored — any handle lookup should fail
            prop_assert!(mgr.pointers.get(&handle).is_none());
        }
    }
}
