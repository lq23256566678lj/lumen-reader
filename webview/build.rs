#[cfg(windows)]
fn main() {
    let mut resource = winres::WindowsResource::new();
    resource.set_icon("../native/installer/lumen.ico");
    if let Err(error) = resource.compile() {
        panic!("failed to embed Windows icon: {error}");
    }
}

#[cfg(not(windows))]
fn main() {}
