//! Lazy images: a poster is only downloaded once its card is (nearly) on
//! screen. Cards report themselves while painting; the 250 ms poller turns
//! those reports into a few downloads at a time.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::ui::poster;
use crate::view::MediaHubView;

/// Downloads in flight at once (the service also caps its own parallelism)
const MAX_IN_FLIGHT: usize = 8;
/// Prefetch margin around the window, in logical pixels
const MARGIN: f32 = 240.0;

#[derive(Default)]
pub struct LazyImages {
    /// url → local file (None = failed, don't retry this session)
    ready: HashMap<String, Option<PathBuf>>,
    in_flight: HashSet<String>,
    /// urls of cards painted on screen since the last pump
    wanted: Rc<RefCell<Vec<String>>>,
    pub fetched: usize,
}

impl LazyImages {
    pub fn get(&self, url: &str) -> Option<PathBuf> {
        self.ready.get(url).cloned().flatten()
    }
}

impl MediaHubView {
    /// Ask for an image that isn't tied to a painted poster (hero backdrops)
    pub(crate) fn want_image(&self, url: &str) {
        if !self.lazy.ready.contains_key(url) {
            self.lazy.wanted.borrow_mut().push(url.to_string());
        }
    }
}

impl MediaHubView {
    /// Poster that downloads itself when it scrolls into view. `have` is a
    /// file we already know about (library poster, cached card).
    pub(crate) fn lazy_poster(&self, seed: &str, have: Option<PathBuf>, url: Option<String>, glyph: Option<&str>) -> Div {
        let path = have.or_else(|| url.as_deref().and_then(|u| self.lazy.get(u)));
        let pending = match (&path, url) {
            (None, Some(u)) if !self.lazy.ready.contains_key(&u) => Some(u),
            _ => None,
        };
        poster(seed, path, glyph).when_some(pending, |d, url| {
            let wanted = self.lazy.wanted.clone();
            d.child(
                canvas(
                    move |b, window, _| {
                        let vp = window.viewport_size();
                        let m = px(MARGIN);
                        let visible = b.origin.y < vp.height + m
                            && b.origin.y + b.size.height > -m
                            && b.origin.x < vp.width + m
                            && b.origin.x + b.size.width > -m;
                        if visible {
                            wanted.borrow_mut().push(url.clone());
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
        })
    }

    /// Start downloads for posters that were painted on screen
    pub(crate) fn pump_images(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<String> = std::mem::take(&mut *self.lazy.wanted.borrow_mut());
        if wanted.is_empty() {
            return;
        }
        let mut seen = HashSet::new();
        for url in wanted {
            if self.lazy.in_flight.len() >= MAX_IN_FLIGHT {
                break;
            }
            if !seen.insert(url.clone()) || self.lazy.ready.contains_key(&url) || self.lazy.in_flight.contains(&url) {
                continue;
            }
            // a previous session may have it on disk already
            if let Some(p) = self.svc.cached_image(&url) {
                self.lazy.ready.insert(url, Some(p));
                cx.notify();
                continue;
            }
            self.lazy.in_flight.insert(url.clone());
            let fut = self.svc.image(url.clone());
            cx.spawn(async move |this, cx| {
                let res = fut.await;
                this.update(cx, |this, cx| {
                    this.lazy.in_flight.remove(&url);
                    this.lazy.fetched += 1;
                    this.lazy.ready.insert(url, res.ok());
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
}
