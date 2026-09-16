use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use vanta::theme::Theme;
use vanta::extension::{Component, Extension, ExtensionMetadata, Page};

pub struct CveFeedComponent;

impl Component for CveFeedComponent {
    fn id(&self) -> &'static str {
        "cve_feed"
    }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" CVE Security Feed ", Style::default().fg(theme.red)))
            .border_style(Style::default().fg(theme.dim));
        
        let content = Paragraph::new("Live CVE Feed...").block(block);
        f.render_widget(content, area);
    }
}

pub struct SecurityExtension;

impl Extension for SecurityExtension {
    fn metadata(&self) -> ExtensionMetadata {
        ExtensionMetadata {
            id: "security",
            name: "Vanta Security Pack",
            author: "Community",
            version: "1.0.0",
            description: "Security monitoring components.",
        }
    }

    fn components(&self) -> Vec<Box<dyn Component>> {
        vec![Box::new(CveFeedComponent)]
    }
}
