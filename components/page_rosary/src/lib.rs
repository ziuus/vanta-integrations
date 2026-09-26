use chrono::{Datelike, Local, Weekday};
use extism_pdk::*;
use serde::Deserialize;
use std::cell::RefCell;
use vanta_ext_sdk::{
    ui::{self, Block, Color, Line, Span, Style, Widget},
    ExtensionMetadata, API_VERSION_BASE,
};

#[derive(Deserialize)]
struct KeyPayload {
    widget: String,
    key: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Language {
    English,
    Malayalam,
}

thread_local! {
    static BEAD_INDEX: RefCell<usize> = RefCell::new(0);
    static LANG: RefCell<Language> = RefCell::new(Language::English);
}

const TOTAL_BEADS: usize = 77; // roughly 7 + 5*14 = 77 steps

fn get_mystery(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon | Weekday::Sat => "Joyful",
        Weekday::Tue | Weekday::Fri => "Sorrowful",
        Weekday::Wed | Weekday::Sun => "Glorious",
        Weekday::Thu => "Luminous",
    }
}

fn get_prayer_text(index: usize, lang: Language) -> (String, String) {
    let day = Local::now().weekday();
    let mystery_type = get_mystery(day);

    let en_cross = "In the name of the Father, and of the Son, and of the Holy Spirit. Amen.";
    let ml_cross = "പിതാവിന്റെയും പുത്രന്റെയും പരിശുദ്ധാത്മാവിന്റെയും നാമത്തിൽ. ആമ്മേൻ.";

    let en_our_father = "Our Father, Who art in heaven, hallowed be Thy name...";
    let ml_our_father = "സ്വർഗ്ഗസ്ഥനായ ഞങ്ങളുടെ പിതാവേ, അങ്ങയുടെ നാമം പൂജിതമാകണമേ...";

    let en_hail_mary = "Hail Mary, full of grace, the Lord is with thee...";
    let ml_hail_mary = "നന്മ നിറഞ്ഞ മറിയമേ സ്വസ്തി, കർത്താവ് അങ്ങയോടുകൂടെ...";

    let en_glory_be = "Glory be to the Father, and to the Son, and to the Holy Spirit.";
    let ml_glory_be = "പിതാവിനും പുത്രനും പരിശുദ്ധാത്മാവിനും സ്തുതി.";

    let en_creed = "I believe in God, the Father Almighty, Creator of heaven and earth...";
    let ml_creed = "സർവ്വശക്തനായ പിതാവും ആകാശത്തിന്റെയും ഭൂമിയുടെയും സ്രഷ്ടാവുമായ ദൈവത്തിൽ ഞാൻ വിശ്വസിക്കുന്നു...";

    let en_fatima = "O my Jesus, forgive us our sins, save us from the fires of hell...";
    let ml_fatima = "ഓ എന്റെ ഈശോയെ, ഞങ്ങളുടെ പാപങ്ങൾ പൊറുക്കണമേ, നരകാഗ്നിയിൽ നിന്ന് ഞങ്ങളെ രക്ഷിക്കണമേ...";

    let en_queen = "Hail, Holy Queen, Mother of Mercy, our life, our sweetness and our hope...";
    let ml_queen = "പരിശുദ്ധ രാജ്ഞി, കരുണയുടെ മാതാവേ, സ്വസ്തി...";

    // Very simple mapping for the 77 steps
    if index == 0 { return ("Sign of the Cross".to_string(), if lang == Language::English { en_cross.to_string() } else { ml_cross.to_string() }); }
    if index == 1 { return ("Apostles' Creed".to_string(), if lang == Language::English { en_creed.to_string() } else { ml_creed.to_string() }); }
    if index == 2 { return ("Our Father".to_string(), if lang == Language::English { en_our_father.to_string() } else { ml_our_father.to_string() }); }
    if index >= 3 && index <= 5 { return ("Hail Mary".to_string(), if lang == Language::English { en_hail_mary.to_string() } else { ml_hail_mary.to_string() }); }
    if index == 6 { return ("Glory Be".to_string(), if lang == Language::English { en_glory_be.to_string() } else { ml_glory_be.to_string() }); }

    if index >= 7 && index < 77 {
        let decade = (index - 7) / 14;
        let step = (index - 7) % 14;
        
        let mystery_title = format!("{} Mystery {}", mystery_type, decade + 1);
        
        if step == 0 { return (mystery_title.clone(), if lang == Language::English { format!("Meditate on the {} mystery.", mystery_title) } else { format!("{} രഹസ്യം ധ്യാനിക്കുക.", mystery_title) }); }
        if step == 1 { return ("Our Father".to_string(), if lang == Language::English { en_our_father.to_string() } else { ml_our_father.to_string() }); }
        if step >= 2 && step <= 11 { return (format!("Hail Mary {}/10", step - 1), if lang == Language::English { en_hail_mary.to_string() } else { ml_hail_mary.to_string() }); }
        if step == 12 { return ("Glory Be".to_string(), if lang == Language::English { en_glory_be.to_string() } else { ml_glory_be.to_string() }); }
        if step == 13 { return ("Fatima Prayer".to_string(), if lang == Language::English { en_fatima.to_string() } else { ml_fatima.to_string() }); }
    }

    ("Hail Holy Queen".to_string(), if lang == Language::English { en_queen.to_string() } else { ml_queen.to_string() })
}

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(ExtensionMetadata::new(
        "page_rosary",
        "Rosary",
        "0.1.0",
        "Interactive Rosary with beads and ASCII art",
        API_VERSION_BASE,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["rosary_art", "rosary_counter", "rosary_prayer"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn handle_key(payload_str: String) -> FnResult<Vec<u8>> {
    let mut handled = false;
    if let Ok(payload) = serde_json::from_str::<KeyPayload>(&payload_str) {
        if payload.widget == "rosary_counter" {
            handled = true;
            match payload.key.as_str() {
                " " | "enter" | "right" => {
                    BEAD_INDEX.with(|b| {
                        let mut idx = b.borrow_mut();
                        if *idx < TOTAL_BEADS {
                            *idx += 1;
                        }
                    });
                }
                "left" | "h" => {
                    BEAD_INDEX.with(|b| {
                        let mut idx = b.borrow_mut();
                        if *idx > 0 {
                            *idx -= 1;
                        }
                    });
                }
                "l" | "L" => { // oops already matched "l", let's use "m" for malayalam toggle
                    // wait, match arms
                }
                _ => handled = false,
            }
            // Add 'm' for language
            if payload.key == "m" || payload.key == "e" {
                LANG.with(|l| {
                    let mut lang = l.borrow_mut();
                    if *lang == Language::English {
                        *lang = Language::Malayalam;
                    } else {
                        *lang = Language::English;
                    }
                });
                handled = true;
            }
        }
    }
    Ok(serde_json::to_vec(&handled).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    let idx = BEAD_INDEX.with(|b| *b.borrow());
    let lang = LANG.with(|l| *l.borrow());

    let widget = match id.as_str() {
        "rosary_art" => {
            // ASCII Holy Picture
            let art = r#"
      .     +       .
          _ | _
         '\   /' 
         -- * --
         ,/ _ \.
      .     |       .
            |
            |
            +
            "#;
            let lines = art.lines().map(|l| Line::text(l, Style::fg(Color::YELLOW))).collect();
            Widget::paragraph(lines).block(Block::titled(" ✝️ Holy Cross "))
        }
        "rosary_counter" => {
            let progress = (idx as f64 / TOTAL_BEADS as f64) * 100.0;
            
            let mut controls = vec![];
            controls.push(Line::text(format!("Bead: {} / {}", idx, TOTAL_BEADS), Style::fg(Color::WHITE).bold()));
            controls.push(Line::blank());
            controls.push(Line::text(
                if lang == Language::English { "Language: English [Press 'm' to switch]" } else { "ഭാഷ: മലയാളം [Press 'm' to switch]" },
                Style::fg(Color::CYAN)
            ));
            controls.push(Line::blank());
            controls.push(Line::text("Controls:", Style::fg(Color::DARK_GRAY)));
            controls.push(Line::text(" [Space/Right] Next Bead", Style::fg(Color::GRAY)));
            controls.push(Line::text(" [Left] Previous Bead", Style::fg(Color::GRAY)));
            controls.push(Line::text(" [m] Toggle English/Malayalam", Style::fg(Color::GRAY)));

            Widget::paragraph(controls).block(Block::titled(" Controls "))
        }
        "rosary_prayer" => {
            let (title, text) = get_prayer_text(idx, lang);
            
            let mut p_lines = vec![];
            p_lines.push(Line::text(title, Style::fg(Color::GREEN).bold()));
            p_lines.push(Line::blank());
            
            // Wrap text manually or use widget wrap
            p_lines.push(Line::text(text, Style::fg(Color::WHITE)));

            Widget::Paragraph {
                lines: p_lines,
                block: Some(Block::titled(" Current Prayer ")),
                wrap: true,
            }
        }
        _ => return Ok(ui::unavailable("UNKNOWN", "invalid widget").to_json()),
    };

    Ok(widget.to_json())
}
