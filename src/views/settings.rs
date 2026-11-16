//! Settings: the Garmin account, the maps, the data received, the theme and language. A sheet
//! over the ground, in two columns when there is room.

use crate::data::{Data, Status};
use crate::logic::regions::{self, Region};
use crate::lang::{tr, trf};
use crate::platform::secrets::Store;
use crate::platform::{store, tiles};
use crate::ui::theme::{MONO, SERIF};
use crate::ui::{button, segmented};
use crate::views::{Width, tracks_progress};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use std::sync::Arc;
use gpui_kit::{prelude::FluentBuilder, *};

/// What the settings file keeps for each theme, in the order they are shown.
const THEMES: [&str; 3] = ["system", "light", "dark"];

/// Below this width, one column.
const TWO_COLUMNS: f32 = 940.0;

pub struct SettingsView {
    data: Entity<Data>,
    width: Width,
    key_input: Entity<InputState>,
    tiles_input: Entity<InputState>,
    /// A region's name, to find it in Geofabrik's list.
    region_input: Entity<InputState>,
    /// The key being changed: its field shows (always, without a key).
    changing_key: bool,
    _subscriptions: [Subscription; 2],
}

impl SettingsView {
    pub fn new(data: Entity<Data>, width: Width, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let key_input = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("GCDC_…"));
        let saved = data.read(cx).settings.get("tiles").cloned().unwrap_or_default();
        let tiles_input = cx.new(|cx| InputState::new(window, cx).placeholder(tiles::DEFAULT_TILES).default_value(saved));
        let region_input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("A region or a country: Bretagne, Switzerland…", "Une région ou un pays : Bretagne, Suisse…")));
        let observe = cx.observe(&data, |_, _, cx| cx.notify());
        // The list is fetched once a name is typed.
        let typed = cx.subscribe(&region_input, |this, input, e: &InputEvent, cx| {
            if let InputEvent::Change = e {
                if !input.read(cx).value().trim().is_empty() {
                    this.data.update(cx, |d, cx| d.load_catalog(cx));
                }
                cx.notify();
            }
        });
        SettingsView { data, width, key_input, tiles_input, region_input, changing_key: false, _subscriptions: [observe, typed] }
    }

    fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.key_input.read(cx).value().to_string();
        if self.data.update(cx, |d, cx| d.save_key(&raw, cx)) {
            self.key_input.update(cx, |s, cx| s.set_value("", window, cx));
            self.changing_key = false;
        }
    }

    /// Puts the cursor in the region's field, as a click in it would.
    #[cfg(test)]
    pub fn focus_region(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.region_input.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Removes a region's files once confirmed.
    fn confirm_remove(&mut self, r: Arc<Region>, window: &mut Window, cx: &mut Context<Self>) {
        let size = store::region_files(&r.id).0.map(size_label).unwrap_or_default();
        let question = trf(format!("Remove {}?", r.name()), format!("Retirer {} ?", r.name()));
        let detail = trf(
            format!("Its extract ({size}) and its path network are deleted from the disk; the loops saved stay. It can be downloaded again."),
            format!("Son extrait ({size}) et son réseau de chemins sont supprimés du disque ; les boucles enregistrées restent. Elle pourra être téléchargée à nouveau."),
        );
        let answer = window.prompt(PromptLevel::Warning, &question, Some(&detail), &[tr("Remove", "Retirer"), tr("Cancel", "Annuler")], cx);
        let data = self.data.clone();
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                data.update(cx, |d, cx| d.remove_region(&r.id, cx));
            }
        })
        .detach();
    }

    fn save_tiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.tiles_input.read(cx).value().to_string();
        if self.data.update(cx, |d, cx| d.save_tiles(&value, cx)) && value.trim().is_empty() {
            self.tiles_input.update(cx, |s, cx| s.set_value("", window, cx));
        }
    }
}

/// "412 Mo", "1,2 Go".
fn size_label(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{} {}", crate::lang::decimal(format!("{:.1}", n as f64 / 1e9)), tr("GB", "Go"))
    } else {
        format!("{} {}", (n as f64 / 1e6).ceil(), tr("MB", "Mo"))
    }
}

/// A section's title, in the serif of the Atelier pages.
fn heading(text: impl Into<SharedString>) -> Div {
    div().font_family(SERIF).font_weight(FontWeight::SEMIBOLD).text_size(px(18.)).child(text.into())
}

fn section() -> Div {
    div().flex().flex_col().gap(px(8.))
}

/// A line of a section: what, and its value at the right.
fn row(label: impl IntoElement, value: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .min_h(px(26.))
        .child(div().flex_none().child(label))
        .child(div().min_w_0().child(value))
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.get(window);
        let t = self.data.read(cx).theme;
        let two = width >= TWO_COLUMNS;
        let column = || div().flex_1().min_w_0().flex().flex_col().gap(px(28.));
        let left = column().child(self.account(cx)).child(self.maps(cx));
        let right = column().child(self.received(cx)).child(self.not_read(cx)).child(self.appearance(cx));
        let body = div()
            .w_full()
            .max_w(px(1120.))
            .mx_auto()
            .flex()
            .gap(px(if two { 44. } else { 28. }))
            .map(|d| if two { d.items_start() } else { d.flex_col() })
            .child(left)
            .child(right);
        // The ground around the sheet, as under the rail.
        div().size_full().bg(t.panel).p(px(8.)).pl_0().child(
            div()
                .id("settings")
                .size_full()
                .rounded(px(12.))
                .bg(t.bg)
                .shadow_sm()
                .overflow_y_scroll()
                .track_scroll(self.width.scroll())
                .child(
                    div()
                        .px(px(crate::ui::pad(width)))
                        .pt(px((crate::ui::top() - 8.0).max(20.)))
                        .pb(px(48.))
                        .flex()
                        .flex_col()
                        .gap(px(24.))
                        .child(
                            div()
                                .w_full()
                                .max_w(px(1120.))
                                .mx_auto()
                                .font_family(SERIF)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(30.))
                                .child(tr("Settings", "Réglages")),
                        )
                        .child(body),
                ),
        )
    }
}

impl SettingsView {
    /// The key (where it is kept, or the field to give one), the synchronisation and its files.
    fn account(&mut self, cx: &mut Context<Self>) -> Div {
        let d = self.data.read(cx);
        let t = d.theme;
        let has = d.key.is_some();
        let no_store = if let Store::Vault(why) = &d.key_store { Some(why.clone()) } else { None };
        let key_store = crate::logic::system::key_store(std::env::consts::OS);
        let checking = d.busy() && d.key_error.is_none();
        let key_error = d.key_error.clone();
        let editing = self.changing_key || !has;
        let (syncing, has_acts) = (d.syncing(), !d.acts.is_empty());
        let where_key = match (has, &no_store) {
            (false, _) => div().text_color(t.soft).child(tr("none", "aucune")),
            (true, Some(_)) => div().text_color(t.good().1).child(tr("encrypted by the app", "chiffrée par l'app")),
            (true, None) => div().text_color(t.good().1).child(key_store.pill),
        };
        let sync_state = if d.listing() {
            match &d.status {
                Status::Listing(s) => s.clone(),
                _ => String::new(),
            }
        } else if d.fetch.running {
            tr("downloading the files", "téléchargement des fichiers").into()
        } else {
            d.note.clone().unwrap_or_else(|| tr("not started yet", "pas encore lancée").into())
        };
        let soft = |text: SharedString| div().text_size(px(12.5)).line_height(rems(1.5)).text_color(t.soft).child(text);
        section()
            .child(heading(tr("Garmin account", "Compte Garmin")))
            .child(row(tr("Personal API key", "Clé de l'API personnelle"), where_key))
            // Said once, here, rather than at each launch.
            .when_some(no_store, |s, why| {
                s.child(soft(trf(
                    format!("{why} {}: the key is encrypted in an app file, and the key that opens it is kept apart, in your configuration folder; one is useless without the other.", key_store.missing),
                    format!("{why} {} : la clé est chiffrée dans un fichier de l'app, et la clé qui l'ouvre est gardée à part, dans ton dossier de configuration ; l'une sans l'autre ne sert à rien.", key_store.missing),
                ).into()))
            })
            .child(row(tr("Synchronisation", "Synchronisation"), div().text_color(t.soft).text_right().child(sync_state)))
            .when(has_acts, |s| s.child(tracks_progress(&self.data, cx)))
            .when(has && !editing, |s| {
                s.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .mt(px(4.))
                        .when(!syncing, |d| d.child(button("sync-now", tr("Synchronise", "Synchroniser"), false, t).on_click(cx.listener(|this, _, _, cx| this.data.update(cx, |d, cx| d.sync(cx))))))
                        .child(button("change-key", tr("Change the key", "Changer de clé"), false, t).on_click(cx.listener(|this, _, _, cx| {
                            this.changing_key = true;
                            cx.notify();
                        }))),
                )
            })
            .when(editing, |s| {
                s.child(soft(
                    tr(
                        "Garmin Connect, on the web: Account settings › Personal API key. It is shown only once: paste it here straight away.",
                        "Garmin Connect, sur le web : Paramètres du compte › Clé API personnelle. Elle ne s'affiche qu'une fois : colle-la ici directement.",
                    )
                    .into(),
                ))
                .child(Input::new(&self.key_input).mask_toggle())
                .child(match &key_error {
                    Some(e) => div().text_size(px(12.)).text_color(t.bad()).child(e.clone()),
                    None => div().text_size(px(12.)).text_color(t.dim).child(tr("Checked with Garmin before being saved.", "Vérifiée auprès de Garmin avant d'être enregistrée.")),
                })
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(
                            button("save-key", if checking { tr("Checking…", "Vérification…") } else { tr("Check and save", "Vérifier et enregistrer") }, true, t)
                                .when(checking, |d| d.opacity(0.6))
                                .on_click(cx.listener(|this, _, window, cx| this.save_key(window, cx))),
                        )
                        .when(has, |d| {
                            d.child(button("cancel-key", tr("Cancel", "Annuler"), false, t).on_click(cx.listener(|this, _, window, cx| {
                                this.changing_key = false;
                                this.key_input.update(cx, |s, cx| s.set_value("", window, cx));
                                cx.notify();
                            })))
                            .child(button("delete-key", tr("Remove the key", "Retirer la clé"), false, t).on_click(cx.listener(|this, _, _, cx| {
                                this.changing_key = false;
                                this.data.update(cx, |d, cx| d.delete_key(cx));
                            })))
                        }),
                )
            })
    }

    /// The regions downloaded for the loops, finding and adding another, and the map
    /// background's tile server.
    fn maps(&mut self, cx: &mut Context<Self>) -> Div {
        self.data.update(cx, |d, _| d.read_regions());
        let d = self.data.read(cx);
        let t = d.theme;
        let soft = |text: SharedString| div().text_size(px(12.5)).line_height(rems(1.5)).text_color(t.soft).child(text);
        let (installed, active, job) = (d.loops.regions.clone(), d.loops.region.id.clone(), d.loops_line());
        let mut list = div().flex().flex_col().gap(px(10.));
        if installed.is_empty() {
            list = list.child(soft(tr("No region downloaded yet: find one below to plan loops in it.", "Aucune région téléchargée : cherches-en une ci-dessous pour y planifier des boucles.").into()));
        }
        for (i, r) in installed.iter().enumerate() {
            let on = r.id == active;
            // ponytail: three file stats per region at each frame; keep them in Data if regions pile up.
            let (size, when, built) = store::region_files(&r.id);
            let facts = match (size, when) {
                (Some(n), Some(at)) => {
                    let day = crate::ui::format::long_date(chrono::DateTime::<chrono::Local>::from(at).date_naive());
                    format!("{} · {day}", size_label(n))
                }
                (Some(n), None) => size_label(n),
                (None, _) => tr("not downloaded", "pas téléchargée").into(),
            };
            let state = match (on, size.is_some(), built) {
                (_, false, _) => None,
                (true, _, _) => Some(tr("used for the loops", "utilisée pour les boucles")),
                (false, _, false) => Some(tr("network built when used", "réseau construit à l'usage")),
                (false, _, true) => None,
            };
            let (use_r, update_r, remove_r) = (r.clone(), r.id.clone(), r.clone());
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(row(
                        div().flex().items_baseline().gap(px(8.)).child(div().when(on, |d| d.font_weight(FontWeight::BOLD)).child(r.name().to_string())).when(!r.parent.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(t.soft).child(r.parent.clone()))),
                        div().font_family(MONO).text_size(px(12.5)).text_color(t.soft).child(facts),
                    ))
                    .when_some(state, |d, s| d.child(div().text_size(px(12.)).text_color(if on { t.good().1 } else { t.soft }).child(s)))
                    .when_some(job.clone().filter(|_| on), |d, (line, share)| {
                        d.child(div().text_size(px(12.)).text_color(t.soft).child(line)).when_some(share, |d, f| {
                            d.child(div().h(px(4.)).w_full().rounded(px(2.)).bg(t.border).child(div().h_full().rounded(px(2.)).bg(t.accent).w(relative(f))))
                        })
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .when(!on, |d| d.child(button(("use-region", i), tr("Use", "Utiliser"), false, t).on_click(cx.listener(move |this, _, _, cx| {
                                this.data.update(cx, |d, cx| d.use_region(use_r.clone(), cx));
                            }))))
                            .when(job.is_some() && on, |d| d.child(button("stop-region", tr("Stop", "Arrêter"), false, t).on_click(cx.listener(|this, _, _, cx| this.data.update(cx, |d, cx| d.stop_loops_job(cx))))))
                            .when(job.is_none() || !on, |d| {
                                d.child(
                                    button(("update-region", i), if size.is_some() { tr("Update", "Mettre à jour") } else { tr("Download", "Télécharger") }, false, t)
                                        .on_click(cx.listener(move |this, _, _, cx| this.data.update(cx, |d, cx| d.update_region(&update_r, cx)))),
                                )
                                .child(button(("remove-region", i), tr("Remove", "Retirer"), false, t).on_click(cx.listener(move |this, _, window, cx| this.confirm_remove(remove_r.clone(), window, cx))))
                            }),
                    ),
            );
        }
        // What the typed name finds in Geofabrik's list, the regions already there left out.
        let query = self.region_input.read(cx).value().to_string();
        let found: Div = if query.trim().is_empty() {
            div()
        } else {
            match &d.loops.catalog {
                None => soft(tr("Reading Geofabrik's list of regions…", "Lecture de la liste des régions de Geofabrik…").into()),
                Some(Err(e)) => div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().text_size(px(12.5)).text_color(t.bad()).child(e.clone()))
                    .child(button("catalog-again", tr("Try again", "Réessayer"), false, t).on_click(cx.listener(|this, _, _, cx| this.data.update(cx, |d, cx| d.load_catalog(cx))))),
                Some(Ok(all)) => {
                    let hits: Vec<Region> = regions::search(all, &query, 12).into_iter().filter(|r| !installed.iter().any(|k| k.id == r.id)).take(6).cloned().collect();
                    if hits.is_empty() {
                        soft(tr("No region by that name.", "Aucune région de ce nom.").into())
                    } else {
                        let mut list = div().flex().flex_col().gap(px(2.));
                        for (i, r) in hits.into_iter().enumerate() {
                            let parent = r.parent.clone();
                            let name = r.name().to_string();
                            list = list.child(row(
                                div().flex().items_baseline().gap(px(8.)).child(name).when(!parent.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(t.soft).child(parent))),
                                button(("add-region", i), tr("Add", "Ajouter"), false, t).on_click(cx.listener(move |this, _, window, cx| {
                                    this.data.update(cx, |d, cx| d.add_region(r.clone(), cx));
                                    this.region_input.update(cx, |s, cx| s.set_value("", window, cx));
                                })),
                            ));
                        }
                        list
                    }
                }
            }
        };
        section()
            .child(heading(tr("Maps", "Cartes")))
            .child(soft(
                tr(
                    "The loops are planned on a region's OpenStreetMap extract, downloaded once from Geofabrik, with its network of paths built on this machine.",
                    "Les boucles se planifient sur l'extrait OpenStreetMap d'une région, téléchargé une fois chez Geofabrik, avec son réseau de chemins construit sur cette machine.",
                )
                .into(),
            ))
            .child(list)
            .child(div().mt(px(8.)).text_size(px(12.)).text_color(t.soft).child(tr("Add a region", "Ajouter une région")))
            .child(Input::new(&self.region_input).cleanable(true))
            .child(found)
            .child(div().mt(px(8.)).text_size(px(12.)).text_color(t.soft).child(tr("Tile address ({z}, {x}, {y})", "Adresse des tuiles ({z}, {x}, {y})")))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.tiles_input).cleanable(true)))
                    .child(button("save-tiles", tr("Save", "Enregistrer"), false, t).on_click(cx.listener(|this, _, window, cx| this.save_tiles(window, cx)))),
            )
            .child(soft(
                tr(
                    "The map background: OpenStreetMap's tiles by default, kept on disk so they are asked for only once. Another server's address works too (with your own key if it asks for one).",
                    "Le fond de carte : les tuiles d'OpenStreetMap par défaut, gardées sur le disque pour ne les demander qu'une fois. L'adresse d'un autre serveur marche aussi (avec ta propre clé s'il en demande une).",
                )
                .into(),
            ))
    }

    /// What was received and where it is kept.
    fn received(&self, cx: &mut Context<Self>) -> Div {
        let d = self.data.read(cx);
        let t = d.theme;
        let g = |n: usize| crate::logic::activity::group(n as i64);
        let value = |s: String| div().font_family(MONO).text_size(px(12.5)).text_color(t.soft).text_right().child(s);
        let first = d.acts.iter().filter_map(|a| a.date()).min();
        let outings = match first {
            Some(day) => trf(format!("{} since {}", g(d.acts.len()), crate::ui::format::long_date(day)), format!("{} depuis le {}", g(d.acts.len()), crate::ui::format::long_date(day))),
            None => tr("none yet", "aucune pour l'instant").into(),
        };
        let files = format!("{} / {}", g(d.derived.len().min(d.acts.len())), g(d.acts.len()));
        let dir = {
            let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
            crate::logic::system::tilde(&store::data_dir().display().to_string(), &home)
        };
        section()
            .gap(px(4.))
            .child(div().mb(px(4.)).child(heading(tr("Data received", "Données reçues"))))
            .child(row(tr("Activities", "Sorties"), value(outings)))
            .child(row(tr("FIT files read", "Fichiers FIT lus"), value(files)))
            // A long path is cut, whole in its tooltip.
            .child(row(
                tr("Folder", "Dossier"),
                value(dir.clone())
                    .truncate()
                    .id("data-dir")
                    .tooltip_show_delay(crate::ui::TIP_DELAY)
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(dir.clone()).build(window, cx)),
            ))
            .child(div().mt(px(4.)).text_size(px(12.5)).line_height(rems(1.5)).text_color(t.soft).child(tr(
                "Everything is kept on this disk, readable by you alone; nothing received is erased. The key only goes to personal.connectapi.garmin.com, over HTTPS; map tiles are asked for with nothing that identifies you.",
                "Tout est gardé sur ce disque, lisible par toi seul ; rien de ce qui est reçu n'est effacé. La clé ne part que vers personal.connectapi.garmin.com, en HTTPS ; les tuiles de carte sont demandées sans rien qui t'identifie.",
            )))
    }

    /// What the key gives access to but the app doesn't ask for, and what the API doesn't give.
    fn not_read(&self, cx: &mut Context<Self>) -> Div {
        let t = self.data.read(cx).theme;
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(16.))
            .py(px(14.))
            .rounded(px(10.))
            .bg(t.panel)
            .child(div().font_weight(FontWeight::BOLD).child(tr("What Astrolabe doesn't read", "Ce qu'Astrolabe ne lit pas")))
            .child(div().text_size(px(13.)).line_height(rems(1.55)).text_color(t.soft).child(tr(
                "Menstrual cycle, weight and nutrition: the key gives access to them, but Astrolabe doesn't ask for them. No VO2max, training status or race predictions: the personal API doesn't provide them.",
                "Cycle menstruel, poids et nutrition : la clé y donne accès, mais Astrolabe ne les demande pas. Pas de VO2max, d'état d'entraînement ni de prédictions de course : l'API personnelle ne les fournit pas.",
            )))
    }

    /// Theme and language: the system's by default, or one picked here.
    fn appearance(&self, cx: &mut Context<Self>) -> Div {
        let d = self.data.read(cx);
        let t = d.theme;
        let saved = |key: &str| d.settings.get(key).cloned().unwrap_or_default();
        let items = |values: &[&str], labels: [&'static str; 3], current: &str| -> Vec<(SharedString, bool)> {
            // Anything unknown in the file is the system's choice, the first.
            let at = values.iter().position(|v| *v == current).unwrap_or(0);
            labels.into_iter().enumerate().map(|(i, l)| (l.into(), i == at)).collect()
        };
        let theme = items(&THEMES, [tr("System", "Système"), tr("Light", "Clair"), tr("Dark", "Sombre")], &saved("theme"));
        let [(en, english), (fr, french)] = crate::lang::LANGUAGES;
        let codes = ["system", en, fr];
        let language = items(&codes, [tr("System", "Système"), english, french], &saved("language"));
        section()
            .child(heading(tr("Appearance", "Apparence")))
            .child(row(
                tr("Theme", "Thème"),
                segmented("settings-theme", theme, t, cx, |this: &mut SettingsView, i, window, cx| {
                    this.data.update(cx, |d, cx| {
                        d.set_setting("theme", THEMES[i].into(), cx);
                        d.follow_theme(window, cx);
                    });
                }),
            ))
            .child(row(
                tr("Language", "Langue"),
                segmented("settings-language", language, t, cx, move |this: &mut SettingsView, i, _, cx| {
                    crate::lang::choose(Some(codes[i]));
                    this.data.update(cx, |d, cx| d.set_setting("language", codes[i].into(), cx));
                    cx.refresh_windows();
                }),
            ))
    }
}
