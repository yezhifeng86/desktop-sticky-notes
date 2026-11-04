//! 把应用图标和版本信息嵌进 exe（GNU 工具链下 winres 会调用 windres）。

fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/app.ico");
    res.set("ProductName", "桌面便利贴");
    res.set("FileDescription", "桌面便利贴 - 单文件桌面计划便签");
    res.set("InternalName", "desktop-sticky-notes");
    res.set("OriginalFilename", "桌面便利贴.exe");
    res.set("LegalCopyright", "MIT © 青山白马少年");
    res.set("ProductVersion", "1.0.0");
    res.set("FileVersion", "1.0.0");
    // 让 windres 重新生成（图标改动时）
    println!("cargo:rerun-if-changed=assets/app.ico");
    res.compile().expect("嵌入图标/版本资源失败");

    // winres 走的是 `link-lib=static=resource`，但 windres 产出的对象没有任何符号，
    // 链接器不会把归档成员拉进来 → 结果 exe 里根本没有 .rsrc 段。
    // 这里直接把 .o 当链接参数传进去，资源才会真的进 PE。
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let obj = std::path::Path::new(&out_dir).join("resource.o");
    if obj.exists() {
        println!("cargo:rustc-link-arg={}", obj.to_string_lossy().replace('\\', "/"));
    } else {
        println!("cargo:warning=windres 未产出 resource.o，图标未嵌入");
    }
}
