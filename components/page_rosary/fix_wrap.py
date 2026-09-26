with open("src/lib.rs", "r") as f:
    text = f.read()

text = text.replace("""            Widget::paragraph(p_lines)
                .wrap(true)
                .block(Block::titled(" Current Prayer "))""",
"""            Widget::Paragraph {
                lines: p_lines,
                block: Some(Block::titled(" Current Prayer ")),
                wrap: true,
            }""")

with open("src/lib.rs", "w") as f:
    f.write(text)
