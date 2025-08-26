fn main() {
    #[cfg(target_os = "windows")]
    {
        // Only compile resource files on Windows
        println!("cargo:rerun-if-changed=src/html_src/icon.ico");

        // Compile the resource file only on Windows using winres crate
        let mut res = winres::WindowsResource::new();
        res.set_icon("src/html_src/icon.ico");
        res.compile().unwrap();
    }
}
