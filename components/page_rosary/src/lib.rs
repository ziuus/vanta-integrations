use extism_pdk::*;
use std::cell::RefCell;
use vanta_ext_sdk::{
    ui::{self, Block, Color, Line, Span, Style, Widget},
    ExtensionMetadata,
};

#[derive(serde::Deserialize)]
struct KeyPayload {
    widget: String,
    key: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Language {
    English,
    Malayalam,
}

#[derive(Clone, Copy, PartialEq)]
enum Mystery {
    Joyful,
    Sorrowful,
    Glorious,
    Luminous,
}

impl Mystery {
    fn title(&self) -> &'static str {
        match self {
            Mystery::Joyful => "Joyful Mysteries",
            Mystery::Sorrowful => "Sorrowful Mysteries",
            Mystery::Glorious => "Glorious Mysteries",
            Mystery::Luminous => "Luminous Mysteries",
        }
    }
    fn next(&self) -> Self {
        match self {
            Mystery::Joyful => Mystery::Sorrowful,
            Mystery::Sorrowful => Mystery::Glorious,
            Mystery::Glorious => Mystery::Luminous,
            Mystery::Luminous => Mystery::Joyful,
        }
    }
    fn get_decade(&self, decade: usize, lang: Language) -> (String, String) {
        let (en_title, en_desc, ml_title, ml_desc) = match (self, decade) {
            (Mystery::Joyful, 0) => ("The Annunciation", "The Angel Gabriel announces to Mary that she will conceive the Son of God.", "മംഗളവാർത്ത", "ഗബ്രിയേൽ മാലാഖ പരിശുദ്ധ കന്യകാമറിയത്തെ മംഗളവാർത്ത അറിയിക്കുന്നു."),
            (Mystery::Joyful, 1) => ("The Visitation", "Mary visits her cousin Elizabeth, who is pregnant with John the Baptist.", "എലിശ്വായെ സന്ദർശിക്കുന്നത്", "പരിശുദ്ധ കന്യകാമറിയം എലിശ്വായെ സന്ദർശിക്കുന്നു."),
            (Mystery::Joyful, 2) => ("The Nativity", "Jesus is born in a stable in Bethlehem.", "ഈശോയുടെ ജനനം", "ഈശോ ബെത്‌ലഹേമിൽ ജനിക്കുന്നു."),
            (Mystery::Joyful, 3) => ("The Presentation", "Mary and Joseph present Jesus in the Temple.", "ദേവാലയത്തിൽ കാഴ്ചവയ്ക്കുന്നത്", "മാതാപിതാക്കൾ ഈശോയെ ദേവാലയത്തിൽ കാഴ്ചവയ്ക്കുന്നു."),
            (Mystery::Joyful, 4) => ("The Finding in the Temple", "The boy Jesus is found in the Temple discussing with the teachers.", "ദേവാലയത്തിൽ കണ്ടെത്തുന്നത്", "കാണാതായ ഈശോയെ ദേവാലയത്തിൽ കണ്ടെത്തുന്നു."),

            (Mystery::Luminous, 0) => ("The Baptism of Christ", "Jesus is baptized in the Jordan by John.", "യോർദ്ദാൻ നദിയിലെ മാമ്മോദീസാ", "ഈശോ യോർദ്ദാൻ നദിയിൽ വെച്ച് മാമ്മോദീസ സ്വീകരിക്കുന്നു."),
            (Mystery::Luminous, 1) => ("The Wedding at Cana", "Jesus turns water into wine, His first public miracle.", "കാനായിലെ കല്യാണം", "കാനായിലെ കല്യാണവിരുന്നിൽ ഈശോ വെള്ളം വീഞ്ഞാക്കുന്നു."),
            (Mystery::Luminous, 2) => ("Proclamation of the Kingdom", "Jesus calls for repentance and proclaims the Kingdom of God.", "ദൈവരാജ്യ പ്രഖ്യാപനം", "ഈശോ ദൈവരാജ്യം പ്രഖ്യാപിക്കുന്നു."),
            (Mystery::Luminous, 3) => ("The Transfiguration", "Jesus is transfigured on Mount Tabor.", "രൂപാന്തരീകരണം", "ഈശോ താബോർ മലയിൽ വെച്ച് രൂപാന്തരപ്പെടുന്നു."),
            (Mystery::Luminous, 4) => ("The Institution of the Eucharist", "Jesus offers His Body and Blood at the Last Supper.", "വിശുദ്ധ കുർബാനയുടെ സ്ഥാപനം", "അവസാന അത്താഴവേളയിൽ ഈശോ വിശുദ്ധ കുർബാന സ്ഥാപിക്കുന്നു."),

            (Mystery::Sorrowful, 0) => ("The Agony in the Garden", "Jesus prays in the Garden of Gethsemane.", "ഗെത്ത്സെമനിയിലെ പ്രാർത്ഥന", "ഈശോ ഗെത്ത്സെമനി തോട്ടത്തിൽ പ്രാർത്ഥിക്കുന്നു."),
            (Mystery::Sorrowful, 1) => ("The Scourging at the Pillar", "Jesus is cruelly scourged.", "ചമ്മട്ടിയടിയേൽക്കുന്നത്", "ഈശോയെ തൂണിൽ കെട്ടി ചമ്മട്ടികൊണ്ടടിക്കുന്നു."),
            (Mystery::Sorrowful, 2) => ("The Crowning with Thorns", "A crown of thorns is placed on the head of Jesus.", "മുൾമുടി ധരിപ്പിക്കുന്നത്", "ഈശോയെ മുൾമുടി ധരിപ്പിക്കുന്നു."),
            (Mystery::Sorrowful, 3) => ("The Carrying of the Cross", "Jesus carries the cross to Calvary.", "കുരിശുവഹിച്ചുള്ള യാത്ര", "ഈശോ കുരിശുവഹിച്ച് കാൽവരിയിലേക്ക് പോകുന്നു."),
            (Mystery::Sorrowful, 4) => ("The Crucifixion", "Jesus is nailed to the cross and dies.", "കുരിശുമരണം", "ഈശോ കുരിശിൽ തറയ്ക്കപ്പെട്ട് മരിക്കുന്നു."),

            (Mystery::Glorious, 0) => ("The Resurrection", "Jesus rises from the dead.", "ഉയിർപ്പ്", "ഈശോ മരിച്ചവരിൽ നിന്നും ഉയിർത്തെഴുന്നേൽക്കുന്നു."),
            (Mystery::Glorious, 1) => ("The Ascension", "Jesus ascends into Heaven.", "സ്വർഗ്ഗാരോഹണം", "ഈശോ സ്വർഗ്ഗത്തിലേക്ക് കരേറുന്നു."),
            (Mystery::Glorious, 2) => ("The Descent of the Holy Spirit", "The Holy Spirit descends upon Mary and the Apostles.", "പരിശുദ്ധാത്മാവിന്റെ ആഗമനം", "പരിശുദ്ധാത്മാവ് ശ്ലീഹന്മാരുടെ മേൽ എഴുന്നള്ളിവരുന്നു."),
            (Mystery::Glorious, 3) => ("The Assumption", "Mary is taken body and soul into Heaven.", "സ്വർഗ്ഗാരോപണം", "പരിശുദ്ധ കന്യകാമറിയത്തെ സ്വർഗ്ഗത്തിലേക്ക് കരേറ്റുന്നു."),
            (Mystery::Glorious, 4) => ("The Coronation", "Mary is crowned Queen of Heaven and Earth.", "സ്വർഗ്ഗരാജ്ഞിയായി മുടിചൂടുന്നത്", "പരിശുദ്ധ കന്യകാമറിയത്തെ സ്വർഗ്ഗരാജ്ഞിയായി മുടിചൂടിക്കുന്നു."),
            
            _ => ("Mystery", "Description", "രഹസ്യം", "വിവരണം")
        };

        let title = format!("{} Mystery {}", self.title(), decade + 1);
        if lang == Language::English {
            (format!("{} - {}", title, en_title), en_desc.to_string())
        } else {
            (format!("{} - {}", title, ml_title), ml_desc.to_string())
        }
    }
}

thread_local! {
    static BEAD_INDEX: RefCell<usize> = RefCell::new(0);
    static LANG: RefCell<Language> = RefCell::new(Language::English);
    static MYST_STATE: RefCell<Mystery> = RefCell::new(Mystery::Joyful);
    static INIT_DONE: RefCell<bool> = RefCell::new(false);
}

// TOTAL_STEPS is 78.
const BEADS: [(usize, usize, bool); 59] = [
    (26, 24, true),
    (26, 23, false),
    (26, 22, false),
    (26, 21, false),
    (26, 20, true),
    (30, 20, false),
    (31, 20, false),
    (33, 19, false),
    (35, 19, false),
    (36, 19, false),
    (38, 18, false),
    (40, 17, false),
    (41, 16, false),
    (43, 15, false),
    (44, 14, false),
    (45, 13, true),
    (46, 11, false),
    (46, 9, false),
    (46, 8, false),
    (45, 6, false),
    (43, 5, false),
    (42, 4, false),
    (40, 3, false),
    (39, 2, false),
    (37, 2, false),
    (35, 1, false),
    (34, 1, true),
    (32, 0, false),
    (30, 0, false),
    (29, 0, false),
    (27, 0, false),
    (25, 0, false),
    (23, 0, false),
    (21, 0, false),
    (20, 1, false),
    (18, 1, false),
    (16, 1, false),
    (15, 2, true),
    (13, 2, false),
    (11, 3, false),
    (10, 4, false),
    (8, 5, false),
    (7, 7, false),
    (6, 8, false),
    (6, 10, false),
    (6, 12, false),
    (7, 13, false),
    (8, 14, false),
    (9, 16, true),
    (11, 17, false),
    (12, 17, false),
    (14, 18, false),
    (16, 19, false),
    (18, 19, false),
    (19, 19, false),
    (21, 20, false),
    (23, 20, false),
    (25, 20, false),
    (26, 20, false),
];
const CHAIN: [(usize, usize); 39] = [
    (28, 20),
    (32, 20),
    (34, 19),
    (37, 18),
    (39, 18),
    (40, 16),
    (42, 16),
    (46, 12),
    (46, 10),
    (46, 7),
    (44, 6),
    (41, 4),
    (40, 2),
    (38, 2),
    (36, 2),
    (33, 0),
    (31, 0),
    (28, 0),
    (26, 0),
    (24, 0),
    (22, 0),
    (20, 0),
    (19, 1),
    (17, 1),
    (16, 2),
    (14, 2),
    (12, 2),
    (9, 4),
    (8, 6),
    (6, 9),
    (6, 11),
    (8, 15),
    (10, 16),
    (13, 18),
    (15, 18),
    (17, 19),
    (20, 20),
    (22, 20),
    (24, 20),
];

const TOTAL_STEPS: usize = 78;

fn step_to_bead(step: usize) -> usize {
    match step {
        0 | 1 => 0, 
        2 => 1,
        3..=5 => step - 1,
        6 => 5,
        7..=76 => {
            let decade = (step - 7) / 14;
            let p = (step - 7) % 14;
            match p {
                0 => 6 + decade * 11, 
                1 => 7 + decade * 11, 
                2..=11 => 6 + decade * 11 + p, 
                _ => 17 + decade * 11, 
            }
        }
        _ => 6, 
    }
}

fn get_prayer_text(index: usize, lang: Language, myst: Mystery) -> (String, String) {
    let en_cross = "In the name of the Father, and of the Son, and of the Holy Spirit. Amen.";
    let ml_cross = "പിതാവിന്റെയും പുത്രന്റെയും പരിശുദ്ധാത്മാവിന്റെയും നാമത്തിൽ. ആമ്മേൻ.";

    let en_our_father = "Our Father, Who art in heaven, hallowed be Thy name; Thy kingdom come; Thy will be done on earth as it is in heaven. Give us this day our daily bread; and forgive us our trespasses as we forgive those who trespass against us; and lead us not into temptation, but deliver us from evil. Amen.";
    let ml_our_father = "സ്വർഗ്ഗസ്ഥനായ ഞങ്ങളുടെ പിതാവേ, അങ്ങയുടെ നാമം പൂജിതമാകണമേ. അങ്ങയുടെ രാജ്യം വരണമേ. അങ്ങയുടെ തിരുമനസ്സ് സ്വർഗ്ഗത്തിലെപ്പോലെ ഭൂമിയിലുമാകണമേ. അന്നന്നുവേണ്ട ആഹാരം ഇന്ന് ഞങ്ങൾക്ക് നൽകണമേ. ഞങ്ങളോട് തെറ്റ് ചെയ്യുന്നവരോട് ഞങ്ങൾ ക്ഷമിക്കുന്നതുപോലെ ഞങ്ങളുടെ തെറ്റുകൾ ഞങ്ങളോടും ക്ഷമിക്കണമേ. ഞങ്ങളെ പ്രലോഭനത്തിൽ ഉൾപ്പെടുത്തരുതേ, തിന്മയിൽ നിന്നും ഞങ്ങളെ രക്ഷിക്കണമേ. ആമ്മേൻ.";

    let en_hail_mary = "Hail Mary, full of grace. The Lord is with thee. Blessed art thou amongst women, and blessed is the fruit of thy womb, Jesus. Holy Mary, Mother of God, pray for us sinners, now and at the hour of our death. Amen.";
    let ml_hail_mary = "നന്മ നിറഞ്ഞ മറിയമേ സ്വസ്തി, കർത്താവ് അങ്ങയോടുകൂടെ. സ്ത്രീകളിൽ അങ്ങ് അനുഗ്രഹിക്കപ്പെട്ടവളാകുന്നു. അങ്ങയുടെ ഉദരത്തിന്റെ ഫലമായ ഈശോ അനുഗ്രഹിക്കപ്പെട്ടവനാകുന്നു. പരിശുദ്ധ മറിയമേ, തമ്പുരാന്റെ അമ്മേ, പാപികളായ ഞങ്ങൾക്കുവേണ്ടി ഇപ്പോഴും ഞങ്ങളുടെ മരണസമയത്തും തമ്പുരാനോട് അപേക്ഷിക്കണമേ. ആമ്മേൻ.";

    let en_glory_be = "Glory be to the Father, and to the Son, and to the Holy Spirit. As it was in the beginning, is now, and ever shall be, world without end. Amen.";
    let ml_glory_be = "പിതാവിനും പുത്രനും പരിശുദ്ധാത്മാവിനും സ്തുതി. ആദിയിലെപ്പോലെ ഇപ്പോഴും എപ്പോഴും എന്നേക്കും. ആമ്മേൻ.";

    let en_creed = "I believe in God, the Father Almighty, Creator of heaven and earth, and in Jesus Christ, His only Son, our Lord, who was conceived by the Holy Spirit, born of the Virgin Mary, suffered under Pontius Pilate, was crucified, died and was buried; He descended into hell; on the third day He rose again from the dead; He ascended into heaven, and is seated at the right hand of God the Father Almighty; from there He will come to judge the living and the dead. I believe in the Holy Spirit, the Holy Catholic Church, the communion of Saints, the forgiveness of sins, the resurrection of the body, and life everlasting. Amen.";
    let ml_creed = "സർവ്വശക്തനായ പിതാവും ആകാശത്തിന്റെയും ഭൂമിയുടെയും സ്രഷ്ടാവുമായ ദൈവത്തിൽ ഞാൻ വിശ്വസിക്കുന്നു. അവിടുത്തെ ഏകപുത്രനും ഞങ്ങളുടെ കർത്താവുമായ ഈശോമിശിഹായിലും ഞാൻ വിശ്വസിക്കുന്നു. ഈ പുത്രൻ പരിശുദ്ധാത്മാവിനാൽ ഗർഭസ്ഥനായി, കന്യകാമറിയത്തിൽ നിന്ന് പിറന്ന്, പന്തിയോസ് പീലാത്തോസിന്റെ കാലത്ത് പീഡകൾ സഹിച്ച്, കുരിശിൽ തറയ്ക്കപ്പെട്ട്, മരിച്ച് അടക്കപ്പെട്ടു. പാതാളത്തിൽ ഇറങ്ങി, മരിച്ചവരുടെ ഇടയിൽ നിന്ന് മൂന്നാം നാൾ ഉയിർത്തു, സ്വർഗ്ഗത്തിലേക്ക് എഴുന്നള്ളി, സർവ്വശക്തനായ പിതാവായ ദൈവത്തിന്റെ വലത്തുഭാഗത്ത് ഇരിക്കുന്നു. അവിടെനിന്ന് ജീവിക്കുന്നവരെയും മരിച്ചവരെയും വിധിക്കാൻ വരുമെന്നും ഞാൻ വിശ്വസിക്കുന്നു. പരിശുദ്ധാത്മാവിലും ഞാൻ വിശ്വസിക്കുന്നു. വിശുദ്ധ കത്തോലിക്കാ സഭയിലും, പുണ്യവാന്മാരുടെ ഐക്യത്തിലും, പാപങ്ങളുടെ മോചനത്തിലും, ശരീരത്തിന്റെ ഉയിർപ്പിലും, നിത്യമായ ജീവിതത്തിലും ഞാൻ വിശ്വസിക്കുന്നു. ആമ്മേൻ.";

    let en_fatima = "O my Jesus, forgive us our sins, save us from the fires of hell, lead all souls to Heaven, especially those most in need of Thy mercy.";
    let ml_fatima = "ഓ എന്റെ ഈശോയെ, ഞങ്ങളുടെ പാപങ്ങൾ പൊറുക്കണമേ, നരകാഗ്നിയിൽ നിന്ന് ഞങ്ങളെ രക്ഷിക്കണമേ. എല്ലാ ആത്മാക്കളെയും, പ്രത്യേകിച്ച് അങ്ങയുടെ കാരുണ്യം ഏറ്റവും കൂടുതൽ ആവശ്യമുള്ളവരെയും സ്വർഗ്ഗത്തിലേക്ക് ആനയിക്കണമേ.";

    let en_queen = "Hail, Holy Queen, Mother of Mercy, our life, our sweetness and our hope. To thee do we cry, poor banished children of Eve: to thee do we send up our sighs, mourning and weeping in this valley of tears. Turn then, most gracious Advocate, thine eyes of mercy toward us, and after this our exile, show unto us the blessed fruit of thy womb, Jesus. O clement, O loving, O sweet Virgin Mary!";
    let ml_queen = "പരിശുദ്ധ രാജ്ഞി, കരുണയുടെ മാതാവേ, സ്വസ്തി. ഞങ്ങളുടെ ജീവനും മാധുര്യവും ശരണവുമേ സ്വസ്തി. ഹവ്വായുടെ പുറന്തള്ളപ്പെട്ട മക്കളായ ഞങ്ങൾ അങ്ങയോട് നിലവിളിക്കുന്നു. കണ്ണുനീരിന്റെ ഈ താഴ്‌വരയിൽ വിങ്ങിക്കരഞ്ഞ് അങ്ങയോട് ഞങ്ങൾ നെടുവീർപ്പിടുന്നു. ആകയാൽ ഞങ്ങളുടെ മദ്ധ്യസ്ഥേ, അങ്ങയുടെ കരുണയുള്ള കണ്ണുകൾ ഞങ്ങളുടെ നേരെ തിരിക്കണമേ. ഞങ്ങളുടെ ഈ പ്രവാസത്തിനു ശേഷം അങ്ങയുടെ ഉദരത്തിന്റെ അനുഗൃഹീത ഫലമായ ഈശോയെ ഞങ്ങൾക്ക് കാണിച്ച് തരണമേ. കരുണാമയിയും സ്നേഹമയിയുമായ കന്യകാമറിയമേ! ആമ്മേൻ.";

    let en_closing = "O God, whose only begotten Son, by His life, death, and resurrection, has purchased for us the rewards of eternal life, grant, we beseech Thee, that meditating upon these mysteries of the Most Holy Rosary of the Blessed Virgin Mary, we may imitate what they contain and obtain what they promise, through the same Christ Our Lord. Amen.";
    let ml_closing = "സർവ്വേശ്വരാ, അങ്ങയുടെ ഏകപുത്രൻ തന്റെ ജീവിതവും മരണവും ഉയിർപ്പും വഴി ഞങ്ങൾക്ക് നിത്യരക്ഷ നേടിത്തന്നുവല്ലോ. പരിശുദ്ധ കന്യകാമറിയത്തിന്റെ അതിപരിശുദ്ധ ജപമാലയിലെ ഈ രഹസ്യങ്ങൾ ധ്യാനിക്കുന്ന ഞങ്ങൾ അവയിൽ അടങ്ങിയിരിക്കുന്നവ അനുകരിക്കാനും അവ വാഗ്ദാനം ചെയ്യുന്നത് പ്രാപിക്കാനും അനുഗ്രഹിക്കണമേയെന്ന് അങ്ങയോട് ഞങ്ങൾ അപേക്ഷിക്കുന്നു. ഈ അപേക്ഷകൾ ഞങ്ങളുടെ കർത്താവായ ഈശോമിശിഹാവഴി ഞങ്ങൾക്ക് സാധിച്ചുതരണമേ. ആമ്മേൻ.";

    if index == 0 { return ("Sign of the Cross".to_string(), if lang == Language::English { en_cross.to_string() } else { ml_cross.to_string() }); }
    if index == 1 { return ("Apostles' Creed".to_string(), if lang == Language::English { en_creed.to_string() } else { ml_creed.to_string() }); }
    if index == 2 { return ("Our Father".to_string(), if lang == Language::English { en_our_father.to_string() } else { ml_our_father.to_string() }); }
    if index >= 3 && index <= 5 { return ("Hail Mary".to_string(), if lang == Language::English { en_hail_mary.to_string() } else { ml_hail_mary.to_string() }); }
    if index == 6 { return ("Glory Be".to_string(), if lang == Language::English { en_glory_be.to_string() } else { ml_glory_be.to_string() }); }

    if index >= 7 && index <= 76 {
        let decade = (index - 7) / 14;
        let step = (index - 7) % 14;
        
        if step == 0 { return myst.get_decade(decade, lang); }
        if step == 1 { return ("Our Father".to_string(), if lang == Language::English { en_our_father.to_string() } else { ml_our_father.to_string() }); }
        if step >= 2 && step <= 11 { return (format!("Hail Mary {}/10", step - 1), if lang == Language::English { en_hail_mary.to_string() } else { ml_hail_mary.to_string() }); }
        if step == 12 { return ("Glory Be".to_string(), if lang == Language::English { en_glory_be.to_string() } else { ml_glory_be.to_string() }); }
        if step == 13 { return ("Fatima Prayer".to_string(), if lang == Language::English { en_fatima.to_string() } else { ml_fatima.to_string() }); }
    }

    if index == 77 { return ("Hail Holy Queen & Closing Prayer".to_string(), if lang == Language::English { format!("{}\n\n{}", en_queen, en_closing) } else { format!("{}\n\n{}", ml_queen, ml_closing) }); }

    ("End of Rosary".to_string(), "In the name of the Father, and of the Son, and of the Holy Spirit. Amen.".to_string())
}

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(ExtensionMetadata {
        id: "page_rosary".into(),
        name: "Rosary".into(),
        author: "zius".into(),
        version: "0.1.0".into(),
        api_version: "0.9.2".into(), // requires vanta_query
        description: "Interactive Rosary".into(),
    }.to_json())
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
        if payload.widget == "rosary_counter" || payload.widget == "rosary_prayer" || payload.widget == "rosary_art" {
            handled = true;
            match payload.key.as_str() {
                " " | "enter" | "right" | "j" => {
                    BEAD_INDEX.with(|b| {
                        let mut idx = b.borrow_mut();
                        if *idx < TOTAL_STEPS {
                            *idx += 1;
                        }
                    });
                }
                "left" | "k" | "backspace" => {
                    BEAD_INDEX.with(|b| {
                        let mut idx = b.borrow_mut();
                        if *idx > 0 {
                            *idx -= 1;
                        }
                    });
                }
                "l" | "L" => {
                    LANG.with(|l| {
                        let mut lang = l.borrow_mut();
                        if *lang == Language::English {
                            *lang = Language::Malayalam;
                        } else {
                            *lang = Language::English;
                        }
                    });
                }
                "m" | "M" => {
                    MYST_STATE.with(|m| {
                        let mut myst = m.borrow_mut();
                        *myst = myst.next();
                    });
                }
                _ => handled = false,
            }
        }
    }
    Ok(serde_json::to_vec(&handled).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    INIT_DONE.with(|i| {
        let mut done = i.borrow_mut();
        if !*done {
            #[derive(serde::Deserialize)]
            struct TimeData { weekday: String }
            if let Ok(time_data) = vanta_ext_sdk::telemetry::query::<TimeData>(r#"{"topic":"time"}"#) {
                let myst = match time_data.weekday.as_str() {
                    "Monday" | "Saturday" => Mystery::Joyful,
                    "Tuesday" | "Friday" => Mystery::Sorrowful,
                    "Wednesday" | "Sunday" => Mystery::Glorious,
                    "Thursday" => Mystery::Luminous,
                    _ => Mystery::Joyful,
                };
                MYST_STATE.with(|m| *m.borrow_mut() = myst);
            }
            *done = true;
        }
    });

    let idx = BEAD_INDEX.with(|b| *b.borrow());
    let lang = LANG.with(|l| *l.borrow());
    let myst = MYST_STATE.with(|m| *m.borrow());

    let widget = match id.as_str() {
        "rosary_art" => {
            let mary_art = r#"
                 ..'`''^````'....              .
              .'^,:IIIIl!ii!I;:,"`..            
            .`";!>~~~>iilll!iiii!I:^'           
           .,!>~~_+++_~~~>iI!l!!i!I:,".         
          .,!>~_+_<<__+~~>i!l!llll!iI:,".       
         .!~_+++_<<<<__+~>Illlllll!!I;:^.       
        .!~__++____<++_~~!IlllIIlll!!!I:.       
       .:~_++++_++++++~~>IlllIIIll!!!!I;'.      
      .:~+++++<+++~~~~~>IIlllllIIllllll!;.      
      ;>~+++<<<<+~>!!!!IllllIIll!lllllllI'.     
     'I>~+++<<<<+~!llllIIIlIIIIIlllllllll!^     
     :!~++_<<<<+~>lIIIIIllIIlI!!Illl!!ll!l;.    
    .:!~_++<<++~!IIIlI!llIII!!!iI!!!I!!llll:.   
    .;>~~+++++~!IIII!!!lllll!!!!!II!!IlllllI.   
    .I!~_++++~>IIIlIIllIIIIlllII!!ll!!llllll,   
    .I>~~+++~>IIIIlIIlIIIlllllIIIll!!!llllIl;   
    ,I!~_++_~>!IIllll!i!!!IIlI!!!!lI!lIIIIll;   
    ;l>~~++~>!IIIIl!i::;I!Ill!!!!!Ill!IIIlll!   
    ;l>~+++~>!I!!I;:^..':!Illllll!IIIlllllIl!   
    I!>~~+~>!!!iI;'.   .'i!IIllllllIIllllll!l   
    I>~++~>i!!l!I^       'ilIIlllIIlllllllllI   
    !~_~~>i!!!i;,.       .,iIllllI!lllllllllI   
    >~~~>iII!I:.         ..;IllIII!!lllllllIl   
   .>~_~>!Ii;,..          .:Illll!lIlllllllll.  
   ,>~~>!I!;.             .'I!lllI!!Illllllll.  
   ,>~>!II:.              .,iIIl!!!!IIlllllll.  
   ;>~>i!i^.              .:IllI!!!!!IllllllI.  
   ;>>!!!I'                :Ill!!!!!lIllllllI.  
   ;l>iIi;.               .!lll!!!!!Illlllll!.  
   :l>iI!;.              .,IllIIlllIIlllllll;   
   .l>ili:..          ...',IlllllllIllllllll,   
    I>iII;.  ..',,,;;;;::;iIlllllllIllllllll.   
    ;!iI!' .':iiiiiiiii!!!IllllllllIlllllllI.   
    .liI;. .;iI!!!!!!!IllllllllllllIlllllllI.   
    .I!I, .;lI!llIIIlllllllllllllllllllllllI.   
     :I;. .iIlI!lIIIIllllllllllllllllllllll!.   
     .;^  .illI!IlllIIIIIllllIlllllllllllll;    
      ..  .:llIIIllllllllllIIIIIllllllllllI.    
          .;ll!lIIIIlllIIlllIIll!Illllllll:     
          .,II!III!lIIlI!!IIIlI!!!IIllllll.     
          .,I!IIlI!!llllI!!!!III!!!!Illll:      
          .;!!IIIII!!!!!!!IIII!!II!!!llll.      
           ;I!IIllll!!!!!Illlll!!!II!llIl       
          .;!IlllllllI!!!!llllll!!IIIlll;       
          .:IllllllllII!!!!IlllIIllll!lI.       
          .;IlllllllII!III!lIIl!!IIlIIl;        
          .:lIlllllIIIIIIIlIIIII!!IlIl!.        
          .:lIIllIIIIIlllIlIlIllll!lIl^         
          .:!IlIII!IIIIllllllllllI!lI:          
          .;IlIllIIIllIllllllllllI!!I.          
           ,IIIIIllllllllllllllllII!;.          
           .iI!l!IIllllllllllIIIl!I;.           
            ^!I!!!IlIllllllllIl!I!;.            
            .:IIlIIIIIlIlllllll!i;.             
             .,!Illl!!IIll!llll!,.              
               .,!IIIIIIlIIIl!;.                
                 ..^;!IlIII!;.                  
                     ...''..                     
            "#;
            let mut lines = vec![];
            for l in mary_art.lines() {
                if !l.trim().is_empty() {
                    lines.push(Line::text(l.to_string(), Style::fg(Color::YELLOW)));
                }
            }
            Widget::Paragraph {
                lines,
                block: Some(Block::titled(" Holy Mary, Mother of God ")),
                wrap: false,
            }
        }
        "rosary_counter" => {
            let mut lines = vec![];
            let current_bead = step_to_bead(idx);

            let mut grid_chars = vec![vec![' '; 95]; 25];
            let mut grid_styles = vec![vec![Style::fg(Color::GRAY); 95]; 25];
            
            for &(cx, cy) in CHAIN.iter() {
                if cx < 95 && cy < 25 {
                    grid_chars[cy][cx] = '·';
                    grid_styles[cy][cx] = Style::fg(Color::DARK_GRAY);
                }
            }

            for (i, &(bx, by, large)) in BEADS.iter().enumerate() {
                if bx < 95 && by < 25 {
                    let mut s = Style::fg(Color::DARK_GRAY);
                    let mut ch = if large { '◉' } else { '○' };
                    
                    if i == 0 { ch = '✝'; }
                    if i == 4 { ch = 'M'; }

                    if i < current_bead {
                        s = Style::fg(Color::CYAN); // Completed
                    } else if i == current_bead {
                        s = Style::fg(Color::YELLOW).bold(); // Current
                        ch = if large { '◉' } else { '●' };
                        if i == 0 { ch = '✞'; }
                    }

                    grid_chars[by][bx] = ch;
                    grid_styles[by][bx] = s;
                }
            }

            let progress = (idx as f64 / TOTAL_STEPS as f64) * 100.0;
            let lang_text = if lang == Language::English { "Eng" } else { "Mal" };
            
            let text1 = format!("{} | {} | Progress: {:.0}%", myst.title(), lang_text, progress);
            let text2 = "[Spc] Next [k] Prev [l] Lang [m] Myst";
            
            let x1 = 26_usize.saturating_sub(text1.chars().count() / 2);
            let x2 = 26_usize.saturating_sub(text2.chars().count() / 2);

            for (i, c) in text1.chars().enumerate() {
                if x1 + i < 95 {
                    grid_chars[8][x1 + i] = c;
                    grid_styles[8][x1 + i] = Style::fg(Color::CYAN);
                }
            }
            for (i, c) in text2.chars().enumerate() {
                if x2 + i < 95 {
                    grid_chars[9][x2 + i] = c;
                    grid_styles[9][x2 + i] = Style::fg(Color::GRAY);
                }
            }

            for y in 0..25 {
                let mut row_spans = vec![];
                let mut empty_count = 0;
                
                for x in 0..95 {
                    let ch = grid_chars[y][x];
                    if ch == ' ' {
                        empty_count += 1;
                    } else {
                        if empty_count > 0 {
                            row_spans.push(ui::span(" ".repeat(empty_count), Style::fg(Color::GRAY)));
                            empty_count = 0;
                        }
                        row_spans.push(ui::span(ch.to_string(), grid_styles[y][x].clone()));
                    }
                }
                if empty_count > 0 {
                    row_spans.push(ui::span(" ".repeat(empty_count), Style::fg(Color::GRAY)));
                }
                lines.push(Line::new(row_spans));
            }

            Widget::Paragraph {
                lines,
                block: Some(Block::titled(" The Rosary ")),
                wrap: false,
            }
        }
        "rosary_prayer" => {
            let (title, text) = get_prayer_text(idx, lang, myst);
            
            let mut p_lines = vec![];
            p_lines.push(Line::blank());
            p_lines.push(Line::text(format!("  {}  ", title), Style::fg(Color::GREEN).bold()));
            p_lines.push(Line::blank());
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
