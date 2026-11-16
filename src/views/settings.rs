//! Settings: the Garmin account, the maps, the data received, the theme and language. A sheet
//! over the ground, in two columns when there is room.

use crate::data::{Data, Status};
use crate::lang::{tr, trf};
use crate::platform::secrets::Store;
use crate::platform::{store, tiles};
use crate::ui::theme::{MONO, SERIF};
use crate::ui::{button, segmented};
use crate::views::{Width, tracks_progress};
use gpui_kit::component::input::{Input, InputState};
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
    /// The key being changed: its field shows (always, without a key).
    changing_key: bool,
    _subscription: Subscription,
}

impl SettingsView {
    pub fn new(data: Entity<Data>, width: Width, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let key_input = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("GCDC_…"));
        let saved = data.read(cx).settings.get("tiles").cloned().unwrap_or_default();
        let tiles_input = cx.new(|cx| InputState::new(window, cx).placeholder(tiles::DEFAULT_TILES).default_value(saved));
        let sub = cx.observe(&data, |_, _, cx| cx.notify());
        SettingsView { data, width, key_input, tiles_input, changing_key: false, _subscription: sub }
    }

    fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.key_input.read(cx).value().to_string();
        if self.data.update(cx, |d, cx| d.save_key(&raw, cx)) {
            self.key_input.update(cx, |s, cx| s.set_value("", window, cx));
            self.changing_key = false;
        }
    }

    fn save_tiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.tiles_input.read(cx).value().to_string();
        if self.data.update(cx, |d, cx| d.save_tiles(&value, cx)) && value.trim().is_empty() {
            self.tiles_input.update(cx, |s, cx| s.set_value("", window, cx));
        }
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
fn row(label: impl Into<SharedString>, value: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .min_h(px(26.))
        .child(div().flex_none().child(label.into()))
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

    /// The map background's tile server.
    fn maps(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.data.read(cx).theme;
        section()
            .child(heading(tr("Maps", "Cartes")))
            .child(div().text_size(px(12.5)).line_height(rems(1.5)).text_color(t.soft).child(tr(
                "Map background: OpenStreetMap's tiles by default, kept on disk so they are asked for only once. Another server's address works too (with your own key if it asks for one).",
                "Fond de carte : les tuiles d'OpenStreetMap par défaut, gardées sur le disque pour ne les demander qu'une fois. L'adresse d'un autre serveur marche aussi (avec ta propre clé s'il en demande une).",
            )))
            .child(div().text_size(px(12.)).text_color(t.soft).child(tr("Tile address ({z}, {x}, {y})", "Adresse des tuiles ({z}, {x}, {y})")))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.tiles_input).cleanable(true)))
                    .child(button("save-tiles", tr("Save", "Enregistrer"), false, t).on_click(cx.listener(|this, _, window, cx| this.save_tiles(window, cx)))),
            )
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
