use cfg_if::cfg_if;

#[allow(unused_macros)]
macro_rules! imp {
    ($file:literal) => {
        #[path = $file]
        mod imp;

        pub fn extract_href(mut code: String) -> Option<String> {
            code.push_str("location.href");
            imp::extract_href(&code)
        }
    };
}

cfg_if! {
    if #[cfg(feature = "v8")] {
        imp!("v8.rs");
    } else if #[cfg(feature = "boa")] {
        imp!("boa.rs");
    } else if #[cfg(any(feature = "quickjs", feature = "quickjs-ng"))] {
        imp!("quickjs.rs");
    } else {
        compile_error!("No js engine selected.");
    }
}
