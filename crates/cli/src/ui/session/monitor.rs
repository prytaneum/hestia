//! Fullscreen resource monitor: live CPU and memory sparklines fed by the
//! daemon's `process.metrics` stream for one supervised process.

use std::collections::VecDeque;

use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Sparkline};
use ratatui::Frame;

use super::{is_cancel, log_header, Flow, Screen};

const HISTORY: usize = 240;

/// One tick's sample for the monitored process; `None` means it reported none
/// (stopped or gone this interval).
pub struct MonitorSample {
    pub cpu_pct: f32,
    pub mem_bytes: u64,
}

pub struct MonitorScreen {
    title: String,
    cpu: VecDeque<u64>,
    mem: VecDeque<u64>,
    last: Option<MonitorSample>,
}

impl MonitorScreen {
    pub fn new(title: &str) -> Self {
        MonitorScreen {
            title: title.to_string(),
            cpu: VecDeque::with_capacity(HISTORY),
            mem: VecDeque::with_capacity(HISTORY),
            last: None,
        }
    }

    fn push(&mut self, sample: MonitorSample) {
        if self.cpu.len() == HISTORY {
            self.cpu.pop_front();
            self.mem.pop_front();
        }
        self.cpu.push_back(sample.cpu_pct.round() as u64);
        self.mem.push_back(sample.mem_bytes / (1024 * 1024));
        self.last = Some(sample);
    }
}

impl Screen for MonitorScreen {
    type Event = Option<MonitorSample>;
    type Outcome = ();

    fn draw(&mut self, frame: &mut Frame) {
        let [header, cpu_area, mem_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .areas(frame.area());

        log_header(frame, header, &self.title, 0, "Esc quit");

        let (cpu_label, mem_label) = match &self.last {
            Some(s) => (
                format!("CPU  {}%", s.cpu_pct.round() as u64),
                format!("MEM  {} MB", s.mem_bytes / (1024 * 1024)),
            ),
            None => ("CPU  —".to_string(), "MEM  —".to_string()),
        };

        let cpu: Vec<u64> = self.cpu.iter().copied().collect();
        frame.render_widget(
            Sparkline::default()
                .block(Block::default().borders(Borders::ALL).title(cpu_label))
                .data(&cpu)
                .style(Style::default().fg(Color::Cyan)),
            cpu_area,
        );

        let mem: Vec<u64> = self.mem.iter().copied().collect();
        frame.render_widget(
            Sparkline::default()
                .block(Block::default().borders(Borders::ALL).title(mem_label))
                .data(&mem)
                .style(Style::default().fg(Color::Green)),
            mem_area,
        );
    }

    fn on_key(&mut self, key: KeyEvent) -> Flow<()> {
        if is_cancel(&key) {
            return Flow::Done(());
        }
        Flow::Continue
    }

    fn on_event(&mut self, sample: Option<MonitorSample>) -> Flow<()> {
        match sample {
            Some(sample) => self.push(sample),
            None => self.last = None,
        }
        Flow::Continue
    }
}
