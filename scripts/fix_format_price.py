import re

with open('/home/zius/Projects/vanta-integrations/cryptopulse/src/lib.rs', 'r') as f:
    code = f.read()

fixed = re.sub(
    r'fn format_price\(p: f64\) -> String \{.*?\n\}',
    '''fn format_price(p: f64) -> String {
    if p < 1.0 {
        format!("{:.4}", p)
    } else {
        format!("{:.2}", p)
    }
}''',
    code,
    flags=re.DOTALL
)

with open('/home/zius/Projects/vanta-integrations/cryptopulse/src/lib.rs', 'w') as f:
    f.write(fixed)
