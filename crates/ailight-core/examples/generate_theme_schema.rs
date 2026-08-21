fn main() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let relative_output = "docs/specs/theme.schema.json";
    let output = workspace.join(relative_output);
    std::fs::write(&output, ailight_core::theme::theme_schema_pretty())
        .unwrap_or_else(|error| panic!("写入 {} 失败: {error}", output.display()));

    let status = std::process::Command::new("pnpm")
        .current_dir(&workspace)
        .args(["exec", "ultracite", "fix", relative_output])
        .status()
        .expect("运行 Ultracite 格式化 Theme Schema 失败；请先安装前端依赖");
    assert!(status.success(), "Ultracite 格式化 Theme Schema 失败");

    println!("generated {}", output.display());
}
