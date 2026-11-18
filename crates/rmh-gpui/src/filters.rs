//! Stream picker filters (resolution, HDR, codec, audio, source, size, sort)
//! and the image-cache settings card.

use gpui_kit::*;
use rmh_app::streams::{toggle, MAX_SIZE_GB, MIN_SIZE_GB};
use rmh_app::{AudioFilter, Codec, HdrFilter, Resolution, Source, StreamFilter, StreamSort};

use crate::theme::Palette;
use crate::view::MediaHubView;

/// Which filter a chip edits: the open picker, or the default in Settings
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterTarget {
    Picker,
    Default,
}

impl FilterTarget {
    fn key(self) -> &'static str {
        match self {
            FilterTarget::Picker => "p",
            FilterTarget::Default => "d",
        }
    }
}

const CACHE_CAPS_MB: [u32; 4] = [100, 250, 500, 1000];

impl MediaHubView {
    pub(crate) fn edit_stream_filter(&mut self, target: FilterTarget, f: impl FnOnce(&mut StreamFilter), cx: &mut Context<Self>) {
        match target {
            FilterTarget::Picker => {
                f(&mut self.stream_filter);
                cx.notify();
            }
            FilterTarget::Default => self.update_config(|c| f(&mut c.stream_filter), false, cx),
        }
    }

    fn filter_group(&self, label: &'static str, pills: Vec<AnyElement>, p: Palette) -> Div {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().w(px(76.0)).flex_none().text_xs().text_color(rgb(p.faint)).child(label))
            .child(div().flex().flex_wrap().gap_2().children(pills))
    }

    #[allow(clippy::too_many_arguments)]
    fn fpill(
        &self,
        target: FilterTarget,
        id: String,
        label: impl Into<SharedString>,
        on: bool,
        p: Palette,
        cx: &mut Context<Self>,
        edit: impl Fn(&mut StreamFilter) + 'static,
    ) -> AnyElement {
        let edit = std::rc::Rc::new(edit);
        self.pill(SharedString::from(format!("sf-{}-{id}", target.key())), label, on, p)
            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                let edit = edit.clone();
                this.edit_stream_filter(target, move |f| edit(f), cx)
            }))
            .into_any_element()
    }

    /// Chip rows for `f`. `full` = every group; otherwise quality, HDR and sort.
    pub(crate) fn stream_filter_panel(&self, f: &StreamFilter, target: FilterTarget, full: bool, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut col = div().flex().flex_col().gap_2();

        let mut res = vec![self.fpill(target, "res-any".into(), "Any", f.resolutions.is_empty(), p, cx, |f| f.resolutions.clear())];
        for r in Resolution::ALL {
            res.push(self.fpill(target, format!("res-{}", r.label()), r.label(), f.resolutions.contains(&r), p, cx, move |f| toggle(&mut f.resolutions, r)));
        }
        col = col.child(self.filter_group("Quality", res, p));

        let hdr = HdrFilter::ALL
            .iter()
            .map(|h| {
                let h = *h;
                self.fpill(target, format!("hdr-{}", h.label()), h.label(), f.hdr == h, p, cx, move |f| f.hdr = h)
            })
            .collect();
        col = col.child(self.filter_group("HDR", hdr, p));

        if full {
            let mut codecs = vec![self.fpill(target, "codec-any".into(), "Any", f.codecs.is_empty(), p, cx, |f| f.codecs.clear())];
            for c in Codec::ALL {
                codecs.push(self.fpill(target, format!("codec-{}", c.label()), c.label(), f.codecs.contains(&c), p, cx, move |f| toggle(&mut f.codecs, c)));
            }
            col = col.child(self.filter_group("Codec", codecs, p));

            let audio = AudioFilter::ALL
                .iter()
                .map(|a| {
                    let a = *a;
                    self.fpill(target, format!("audio-{}", a.label()), a.label(), f.audio == a, p, cx, move |f| f.audio = a)
                })
                .collect();
            col = col.child(self.filter_group("Audio", audio, p));

            let mut sources = vec![self.fpill(target, "src-any".into(), "Any", f.sources.is_empty(), p, cx, |f| f.sources.clear())];
            for s in Source::ALL {
                sources.push(self.fpill(target, format!("src-{}", s.label()), s.label(), f.sources.contains(&s), p, cx, move |f| toggle(&mut f.sources, s)));
            }
            col = col.child(self.filter_group("Source", sources, p));

            let mut size: Vec<AnyElement> = vec![];
            for m in MIN_SIZE_GB {
                let label = m.map(|g| format!("≥ {g} GB")).unwrap_or_else(|| "No min".into());
                size.push(self.fpill(target, format!("min-{m:?}"), label, f.min_gb == m, p, cx, move |f| f.min_gb = m));
            }
            for m in MAX_SIZE_GB {
                let label = m.map(|g| format!("≤ {g} GB")).unwrap_or_else(|| "No max".into());
                size.push(self.fpill(target, format!("max-{m:?}"), label, f.max_gb == m, p, cx, move |f| f.max_gb = m));
            }
            col = col.child(self.filter_group("Size", size, p));

            let other = vec![
                self.fpill(target, "cam".into(), "Hide CAM / TS", f.hide_cam, p, cx, |f| f.hide_cam = !f.hide_cam),
                self.fpill(target, "cached".into(), "⚡ RD cached only", f.cached_only, p, cx, |f| f.cached_only = !f.cached_only),
            ];
            col = col.child(self.filter_group("Show", other, p));
        }

        let sort = StreamSort::ALL
            .iter()
            .map(|s| {
                let s = *s;
                self.fpill(target, format!("sort-{}", s.label()), s.label(), f.sort == s, p, cx, move |f| f.sort = s)
            })
            .collect();
        col.child(self.filter_group("Sort", sort, p))
    }

    /// Settings: default stream filters
    pub(crate) fn stream_filter_card(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let f = self.svc.config().stream_filter;
        self.card(p)
            .child(self.section_title("Stream filters", p))
            .child(div().text_sm().text_color(rgb(p.muted)).child(
                "The stream picker starts with these. Changes made in the picker apply to that search only, unless you press “Save as default” there.",
            ))
            .child(self.stream_filter_panel(&f, FilterTarget::Default, true, p, cx))
            .child(
                div().flex().child(self.button("sf-d-reset", "Reset to defaults", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                    this.edit_stream_filter(FilterTarget::Default, |f| *f = StreamFilter::default(), cx);
                    this.stream_filter = StreamFilter::default();
                }))),
            )
    }

    /// Settings: image cache size and cap
    pub(crate) fn cache_card(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let cap = self.svc.config().image_cache_mb;
        let used = self.image_cache.unwrap_or_default();
        let caps: Vec<AnyElement> = CACHE_CAPS_MB
            .iter()
            .map(|mb| {
                let mb = *mb;
                self.pill(SharedString::from(format!("cache-cap-{mb}")), if mb >= 1000 { "1 GB".to_string() } else { format!("{mb} MB") }, cap == mb, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                        this.update_config(|c| c.image_cache_mb = mb, false, cx);
                        this.trim_image_cache(None, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        self.card(p)
            .child(self.section_title("Image cache", p))
            .child(div().text_sm().text_color(rgb(p.muted)).child(format!(
                "Posters are downloaded only when they scroll into view and kept on disk. Using {:.1} MB in {} files; the least recently used images are removed above the limit (library posters are kept).",
                used.bytes as f64 / 1e6,
                used.files
            )))
            .child(self.filter_group("Limit", caps, p))
            .child(div().flex().child(self.button("cache-clear", "Clear image cache", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                this.trim_image_cache(Some(0), cx);
            }))))
    }

    pub(crate) fn refresh_cache_usage(&mut self) {
        self.image_cache = Some(self.svc.image_cache_usage());
    }

    pub(crate) fn trim_image_cache(&mut self, max_mb: Option<u32>, cx: &mut Context<Self>) {
        let fut = self.svc.prune_image_cache(max_mb);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok((n, bytes)) => this.status = Some(format!("Image cache: removed {n} files ({:.1} MB)", bytes as f64 / 1e6)),
                    Err(e) => this.error = Some(e),
                }
                this.refresh_cache_usage();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

