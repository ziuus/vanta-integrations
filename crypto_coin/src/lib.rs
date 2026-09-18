use extism_pdk::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExtensionMetadata {
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub api_version: String,
}

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    let meta = ExtensionMetadata {
        id: "crypto_coin".to_string(),
        name: "3D Rotating Coin".to_string(),
        author: "zius".to_string(),
        version: "1.0.0".to_string(),
        description: "A rotating 3D coin visualization".to_string(),
        api_version: "0.9.0".to_string(),
    };
    Ok(serde_json::to_vec(&meta)?)
}

#[plugin_fn]
pub fn widgets() -> FnResult<Vec<u8>> {
    let widgets = vec!["coin"];
    Ok(serde_json::to_vec(&widgets)?)
}

static mut FRAME: usize = 0;

const FRAMES: &[&str] = &[
    r#"
      .-------.      
    .'         '.    
   /           _ \   
  |    ₿      (_) |  
  |               |  
   \             /   
    '.         .'    
      `-------`      "#,
    r#"
       .-----.       
     .'       '.     
    /         _ \    
   |   ₿     (_) |   
   |             |   
    \           /    
     '.       .'     
       `-----`       "#,
    r#"
        .---.        
      .'     '.      
     /       _ \     
    |  ₿    (_) |    
    |           |    
     \         /     
      '.     .'      
        `---`        "#,
    r#"
         .-.         
       .'   '.       
      /     _ \      
     | ₿   (_) |     
     |         |     
      \       /      
       '.   .'       
         `-`         "#,
    r#"
          .          
        .' '.        
       /   _ \       
      |₿  (_) |      
      |       |      
       \     /       
        '.'          
          `          "#,
    r#"
          |          
          |          
          |          
          |          
          |          
          |          
          |          
          |          "#,
    r#"
          .          
        .' '.        
       / _   \       
      | (_)  ₿|      
      |       |      
       \     /       
        '.'          
          `          "#,
    r#"
         .-.         
       .'   '.       
      / _     \      
     | (_)   ₿ |     
     |         |     
      \       /      
       '.   .'       
         `-`         "#,
    r#"
        .---.        
      .'     '.      
     / _       \     
    | (_)    ₿  |    
    |           |    
     \         /     
      '.     .'      
        `---`        "#,
    r#"
       .-----.       
     .'       '.     
    / _         \    
   | (_)     ₿   |   
   |             |   
    \           /    
     '.       .'     
       `-----`       "#,
];

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id == "coin" {
        let f = unsafe {
            FRAME = (FRAME + 1) % FRAMES.len();
            FRAME
        };
        
        let mut lines = Vec::new();
        for line in FRAMES[f].lines() {
            if line.is_empty() { continue; }
            lines.push(json!({
                "spans": [ { "content": line, "style": { "fg": "yellow", "bold": true } } ]
            }));
        }
        
        let ui = json!({
            "type": "Paragraph",
            "block": {
                "title": " 🪙 Crypto Coin ",
                "bordered": true,
                "border_color": "yellow"
            },
            "lines": lines,
            "wrap": false
        });
        Ok(serde_json::to_vec(&ui)?)
    } else {
        let err = json!({
            "type": "Paragraph",
            "lines": [ { "spans": [ { "content": "Unknown widget ID" } ] } ],
            "wrap": true
        });
        Ok(serde_json::to_vec(&err)?)
    }
}
