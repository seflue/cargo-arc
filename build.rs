include!("src/js_registry.rs");

fn main() {
    let src_dir = std::path::Path::new("js");

    // 1. Discover: src/*.js (not *.test.js)
    let mut modules = Vec::new();
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(src_dir).expect("src/ directory") {
        let entry = entry.expect("dir entry");
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !is_module_file(&file_name) {
            continue;
        }
        let content = std::fs::read_to_string(entry.path()).expect("read JS file");
        modules.push(parse_js_annotations(&content, &file_name));
        sources.push(content);
    }

    // 2. Validate: @deps match actual source references
    let source_refs: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();
    validate_source_deps(&modules, &source_refs);

    // 3. Topo sort
    let sorted = topo_sort(&modules);

    // 3. Generate
    let code = generate_modules_rs(&modules, &sorted);

    // 4. Write to OUT_DIR
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::write(std::path::Path::new(&out_dir).join("js_modules.rs"), code)
        .expect("write js_modules.rs");

    // 5. Rerun triggers
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/js_registry.rs");
    for entry in std::fs::read_dir(src_dir).expect("src/") {
        let entry = entry.expect("entry");
        let name = entry.file_name().to_string_lossy().to_string();
        if is_module_file(&name) {
            println!("cargo:rerun-if-changed=js/{}", name);
        }
    }
}
