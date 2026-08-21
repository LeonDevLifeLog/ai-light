fn main() {
    let output =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/specs/theme.schema.json");
    std::fs::write(&output, ailight_core::theme::theme_schema_pretty())
        .unwrap_or_else(|error| panic!("写入 {} 失败: {error}", output.display()));
    println!("generated {}", output.display());
}
