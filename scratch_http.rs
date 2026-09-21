use extism_pdk::*;
#[plugin_fn]
pub fn fetch() -> FnResult<String> {
    let req = HttpRequest::new("https://api.coincap.io/v2/assets/bitcoin");
    let res = http::request::<()>(&req, None)?;
    Ok(String::from_utf8(res.body())?)
}
