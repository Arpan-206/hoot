//! Messages: notes sent to this Sprig from the web page's "Send a note"
//! box, newest first. A separate channel from the caption on the photo:
//! notes never appear on the picture, and the photo's caption never
//! appears here. L marks the selected note as seen, which the sender sees
//! as a double tick. The agent's heartbeat keeps the unread count; the
//! menu shows it as a badge.

use hoot_gfx::{CELL_HEIGHT, CELL_WIDTH, Framebuffer, WIDTH};
use hoot_proto::messages::{self, age_label};

use crate::agent;
use crate::apps::{App, AppInfo, Group, Ctx, Transition, back_pressed};
use crate::drivers::input::Button;
use crate::net::{Body, FetchRequest, FetchResult, FixedStr, JobState, Lane, NetState, Sink};
use crate::storage::Config;
use crate::ui::text::{StrBuf, format};
use crate::ui::theme;

pub const INFO: AppInfo = AppInfo { name: "Messages", group: Group::Frame, needs_network: true };

const MAX: usize = 8;
const REFRESH_MS: u32 = 30_000;
/// Two text lines per message plus a gap.
const ROW_H: i32 = CELL_HEIGHT * 2 + 4;
const VISIBLE: usize = ((theme::FOOTER_Y - theme::CONTENT_Y) / ROW_H) as usize;
const COLS: usize = (WIDTH / CELL_WIDTH) as usize - 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
    None,
    List,
    Seen(usize),
}

#[derive(Clone, Copy)]
struct Msg {
    id: u32,
    seen: bool,
    age_secs: u32,
    text: FixedStr<80>,
}

pub struct Messages {
    list: [Msg; MAX],
    count: usize,
    selected: usize,
    first: usize,
    job: Job,
    next_refresh_ms: u32,
    status: StrBuf<20>,
    dirty: bool,
}

fn url(cfg: &Config, path: &str) -> FixedStr<128> {
    let mut u = FixedStr::new();
    u.push_str(cfg.frame_server.as_str().trim_end_matches('/'));
    u.push_str(path);
    u.push_str(cfg.frame_name.as_str());
    u
}

impl Messages {
    pub const fn new() -> Self {
        const EMPTY: Msg = Msg { id: 0, seen: true, age_secs: 0, text: FixedStr::new() };
        Self {
            list: [EMPTY; MAX],
            count: 0,
            selected: 0,
            first: 0,
            job: Job::None,
            next_refresh_ms: 0,
            status: StrBuf::new(),
            dirty: true,
        }
    }

    fn refresh(&mut self, ctx: &mut Ctx) {
        let request = FetchRequest::get(url(ctx.store.config(), "/inbox/"), Sink::Small);
        self.next_refresh_ms = ctx.now_ms.wrapping_add(REFRESH_MS);
        match ctx.net.fetch(request) {
            Ok(()) => self.job = Job::List,
            Err(e) => self.set_status(e.label()),
        }
    }

    fn mark_seen(&mut self, ctx: &mut Ctx, index: usize) {
        let id = self.list[index].id;
        let form: StrBuf<32> = format(format_args!("id={id}"));
        let mut request = FetchRequest::get(url(ctx.store.config(), "/device/seen/"), Sink::Small);
        request.body = Body::Form(FixedStr::truncated(form.as_str()));
        match ctx.net.fetch(request) {
            Ok(()) => self.job = Job::Seen(index),
            Err(e) => self.set_status(e.label()),
        }
    }

    fn set_status(&mut self, text: &str) {
        self.status = format(format_args!("{text}"));
        self.dirty = true;
    }

    fn finish_list(&mut self, ctx: &mut Ctx, r: FetchResult) {
        if r.status != 200 {
            let s: StrBuf<20> = format(format_args!("HTTP {}", r.status));
            self.set_status(s.as_str());
            return;
        }
        let mut list = self.list;
        let mut count = 0;
        ctx.net.small_body_on(Lane::App, |body| {
            let text = core::str::from_utf8(body).unwrap_or("");
            for m in messages::parse(text).take(MAX) {
                list[count] = Msg { id: m.id, seen: m.seen, age_secs: m.age_secs, text: FixedStr::truncated(m.text) };
                count += 1;
            }
        });
        self.list = list;
        self.count = count;
        self.selected = self.selected.min(count.saturating_sub(1));
        agent::set_unread(list[..count].iter().filter(|m| !m.seen).count() as u8);
        self.status.clear();
        self.dirty = true;
    }

    fn draw(&self, fb: &mut Framebuffer, state: NetState) {
        let unread = self.list[..self.count].iter().filter(|m| !m.seen).count();
        let right: StrBuf<12> = if unread > 0 {
            format(format_args!("{unread} new"))
        } else {
            format(format_args!(""))
        };
        theme::screen(fb, "Messages", right.as_str());
        if self.count == 0 {
            let text = if !self.status.is_empty() {
                self.status.as_str()
            } else if !state.is_up() {
                state.label()
            } else if self.job == Job::List {
                "loading..."
            } else {
                "no notes yet"
            };
            fb.draw_text_centered(56, text, theme::MUTED, None, 1);
        }
        let end = (self.first + VISIBLE).min(self.count);
        let mut y = theme::CONTENT_Y;
        for (i, m) in self.list[..end].iter().enumerate().skip(self.first) {
            if i == self.selected {
                fb.fill_rect(2, y - 2, WIDTH - 4, ROW_H, theme::ACCENT_DARK);
                fb.fill_rect(2, y - 2, 2, ROW_H, theme::ACCENT);
            }
            let color = if m.seen { theme::MUTED } else { theme::TEXT };
            let mut age = [0u8; 4];
            let age = age_label(m.age_secs, &mut age);
            fb.draw_text_right(WIDTH - 4, y, age, theme::MUTED, None);
            if !m.seen {
                fb.fill_rect(5, y + 2, 3, 3, theme::ACCENT);
            }
            // Up to two lines of text, wrapped by character count.
            let text = m.text.as_str();
            let (a, b) = split_lines(text, COLS - 4);
            fb.draw_text(10, y, a, color, None);
            fb.draw_text(10, y + CELL_HEIGHT, b, color, None);
            y += ROW_H;
        }
        if !self.status.is_empty() && self.count > 0 {
            fb.draw_text_right(WIDTH - 3, theme::FOOTER_Y, self.status.as_str(), theme::WARN, None);
        } else {
            theme::footer(fb, "L seen   K refresh   J back");
        }
    }
}

/// Split `text` into two display lines of at most `cols` characters each,
/// breaking at a space when one is near.
fn split_lines(text: &str, cols: usize) -> (&str, &str) {
    let n = text.chars().count();
    if n <= cols {
        return (text, "");
    }
    let mut cut = text.char_indices().nth(cols).map_or(text.len(), |(i, _)| i);
    if let Some(space) = text[..cut].rfind(' ')
        && space > cols / 2
    {
        cut = space;
    }
    let rest = text[cut..].trim_start();
    let rest_end = rest.char_indices().nth(cols).map_or(rest.len(), |(i, _)| i);
    (&text[..cut], &rest[..rest_end])
}

impl App for Messages {
    fn info(&self) -> &'static AppInfo {
        &INFO
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.job = Job::None;
        self.status.clear();
        self.dirty = true;
        self.next_refresh_ms = ctx.now_ms;
        ctx.net.request_connect();
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_pressed(ctx.input) {
            return Transition::Exit;
        }
        let now = ctx.now_ms;
        let n = self.count.max(1);
        if ctx.input.repeat(Button::W) || ctx.input.repeat(Button::I) {
            self.selected = (self.selected + n - 1) % n;
            self.dirty = true;
        }
        if ctx.input.repeat(Button::S) {
            self.selected = (self.selected + 1) % n;
            self.dirty = true;
        }
        if self.selected < self.first {
            self.first = self.selected;
        } else if self.selected >= self.first + VISIBLE {
            self.first = self.selected + 1 - VISIBLE;
        }

        let state = ctx.net.state();
        if self.job != Job::None {
            match ctx.net.take_job() {
                JobState::Done(r) => {
                    let job = core::mem::replace(&mut self.job, Job::None);
                    match job {
                        Job::List => self.finish_list(ctx, r),
                        Job::Seen(i) if r.status / 100 == 2 => {
                            self.list[i].seen = true;
                            let unread = self.list[..self.count].iter().filter(|m| !m.seen).count();
                            agent::set_unread(unread as u8);
                            self.dirty = true;
                        }
                        Job::Seen(_) => {
                            let s: StrBuf<20> = format(format_args!("HTTP {}", r.status));
                            self.set_status(s.as_str());
                        }
                        Job::None => {}
                    }
                }
                JobState::Failed(e) => {
                    self.job = Job::None;
                    self.set_status(e.label());
                }
                _ => {}
            }
        } else if state.is_up() {
            if ctx.input.just_pressed(Button::K) || now.wrapping_sub(self.next_refresh_ms) < 1 << 31 {
                self.refresh(ctx);
                self.dirty = true;
            } else if (ctx.input.just_pressed(Button::L) || ctx.input.just_pressed(Button::D))
                && self.count > 0
                && !self.list[self.selected].seen
            {
                self.mark_seen(ctx, self.selected);
            }
        }

        // Redraw on change, and while waiting for the network so the state
        // line stays current.
        if self.dirty || (self.count == 0 && !state.is_up()) {
            self.dirty = false;
            self.draw(ctx.fb, state);
        }
        Transition::Stay
    }
}
