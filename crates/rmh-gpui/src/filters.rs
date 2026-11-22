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

    /// Settings: unified storage budget
    pub(crate) fn cache_card(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = self.svc.config();
        let report = self.storage_report.clone().unwrap_or_default();
        let image_caps: Vec<AnyElement> = CACHE_CAPS_MB
            .iter()
            .map(|mb| {
                let mb = *mb;
                self.pill(SharedString::from(format!("cache-cap-{mb}")), if mb >= 1000 { "1 GB".to_string() } else { format!("{mb} MB") }, cfg.image_cache_mb == mb, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                        this.update_config(|c| c.image_cache_mb = mb, false, cx);
                        this.prune_storage(cx);
                    }))
                    .into_any_element()
            })
            .collect();
        const TORRENT_GB: [u32; 5] = [2, 4, 8, 16, 32];
        let torrent_caps: Vec<AnyElement> = TORRENT_GB
            .iter()
            .map(|gb| {
                let gb = *gb;
                self.pill(SharedString::from(format!("tor-cap-{gb}")), format!("{gb} GB"), cfg.torrent_cache_gb == gb, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                        this.update_config(|c| c.torrent_cache_gb = gb, false, cx);
                        this.prune_storage(cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let bucket = |label: &str, used: rmh_app::CacheUsage| {
            div().text_sm().text_color(rgb(p.muted)).child(format!(
                "{label}: {:.1} MB · {} files",
                used.bytes as f64 / 1e6,
                used.files
            ))
        };
        self.card(p)
            .child(self.section_title("Storage", p))
            .child(div().text_sm().text_color(rgb(p.muted)).child(format!(
                "Total cache {:.1} MB. Caps prune least-recently-used files; torrent orphans are cleaned on startup and when you stop playback.",
                report.total_bytes() as f64 / 1e6
            )))
            .child(bucket("Images", report.images))
            .child(bucket("Torrents", report.torrents))
            .child(bucket("TMDB metadata", report.tmdb))
            .child(bucket("Calendar", report.calendar))
            .child(bucket("Subtitles", report.subtitles))
            .child(self.filter_group("Image limit", image_caps, p))
            .child(self.filter_group("Torrent limit", torrent_caps, p))
            .child(
                self.toggle_row(
                    "tg-lowpower",
                    "Low power mode",
                    "Skip torrent warm-up until Play; prefer quieter background work",
                    cfg.low_power,
                    p,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.low_power = !c.low_power, false, cx))),
            )
            .child(
                self.toggle_row(
                    "tg-debrid-disk",
                    "Prefer debrid when disk is full",
                    "Narrow P2P races when the torrent cache is near its cap",
                    cfg.prefer_debrid_on_disk_pressure,
                    p,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                    this.update_config(|c| c.prefer_debrid_on_disk_pressure = !c.prefer_debrid_on_disk_pressure, false, cx)
                })),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.button("storage-prune", "Prune now", true, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.prune_storage(cx))))
                    .child(self.button("storage-clear-img", "Clear images", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.clear_bucket(rmh_app::StorageBucket::Images, cx);
                    })))
                    .child(self.button("storage-clear-tor", "Clear torrents", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.clear_bucket(rmh_app::StorageBucket::Torrents, cx);
                    }))),
            )
            .child(div().text_sm().text_color(rgb(p.muted)).child(
                "If the app fails to open after an update (migration checksum), use Repair — or quit and run aurivon --repair-db.",
            ))
            .child(
                self.button("storage-repair-db", "Repair library database", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                    this.repair_database(cx);
                })),
            )
    }

    pub(crate) fn repair_database(&mut self, cx: &mut Context<Self>) {
        let fut = self.svc.repair_database();
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(msg) => this.status = Some(msg),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn refresh_cache_usage(&mut self) {
        self.image_cache = Some(self.svc.image_cache_usage());
        self.storage_report = Some(self.svc.storage_report());
    }

    pub(crate) fn prune_storage(&mut self, cx: &mut Context<Self>) {
        let fut = self.svc.prune_all_caches();
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok((n, bytes)) => this.status = Some(format!("Storage: removed {n} files ({:.1} MB)", bytes as f64 / 1e6)),
                    Err(e) => this.error = Some(e),
                }
                this.refresh_cache_usage();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn clear_bucket(&mut self, bucket: rmh_app::StorageBucket, cx: &mut Context<Self>) {
        let fut = self.svc.clear_storage_bucket(bucket);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok((n, bytes)) => this.status = Some(format!("{} cleared: {n} files ({:.1} MB)", bucket.label(), bytes as f64 / 1e6)),
                    Err(e) => this.error = Some(e),
                }
                this.refresh_cache_usage();
                cx.notify();
            })
            .ok();
        })
        .detach();
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

