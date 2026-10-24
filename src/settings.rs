//! Settings: the Garmin key, the map tiles, and where the data goes. A centred column, like a note.

use crate::app::Home;
use crate::garmin;
use crate::ui::{button, heading, icon, pill, title};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::{prelude::FluentBuilder, *};

impl Home {
    pub fn settings(&mut self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let has = self.key.is_some();
        let checking = self.busy() && self.key_error.is_none();
        let fact = |name: IconName, text: String| {
            div()
                .flex()
                .gap(px(10.))
                .child(icon(name, t.soft, 14.).flex_none().mt(px(3.)))
                .child(div().flex_1().min_w_0().text_color(t.soft).child(text))
        };
        let label = |text: &'static str| div().text_size(px(12.)).text_color(t.soft).child(text);
        let section = |gap: f32| div().flex().flex_col().gap(px(gap));

        div().id("settings").size_full().overflow_y_scroll().child(
            div()
                .w_full()
                .max_w(px(760.))
                .mx_auto()
                .px(px(crate::ui::pad(width)))
                .pt(px(crate::ui::top()))
                .pb(px(56.))
                .flex()
                .flex_col()
                .gap(px(36.))
                .child(title("Réglages"))
                .child(
                    section(12.)
                        .child(div().flex().items_center().gap(px(10.)).child(heading("Clé API Garmin")).child(if has {
                            pill("dans le trousseau", t.good())
                        } else {
                            pill("aucune", (t.panel, t.soft))
                        }))
                        .child(div().text_color(t.soft).child(
                            "Garmin Connect, sur le web : Paramètres du compte › Clé API personnelle. Elle ne s'affiche qu'une fois : colle-la ici directement.",
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .mt(px(4.))
                                .child(label(if has { "Remplacer par une nouvelle clé" } else { "Clé API personnelle" }))
                                .child(Input::new(&self.key_input).mask_toggle())
                                .child(match &self.key_error {
                                    Some(e) => div().text_size(px(12.)).text_color(t.bad()).child(e.clone()),
                                    None => div().text_size(px(12.)).text_color(t.dim).child("Vérifiée auprès de Garmin avant d'être enregistrée."),
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    button("save-key", if checking { "Vérification…" } else { "Vérifier et enregistrer" }, true, t)
                                        .when(checking, |d| d.opacity(0.6))
                                        .on_click(cx.listener(|this, _, window, cx| this.save_key(window, cx))),
                                )
                                .when(has, |d| d.child(button("delete-key", "Retirer la clé", false, t).on_click(cx.listener(|this, _, _, cx| this.delete_key(cx))))),
                        ),
                )
                .child(
                    section(12.)
                        .child(heading("Synchronisation"))
                        .child(div().text_color(t.soft).child(
                            "Elle se lance seule à l'ouverture, en deux étapes : la liste de tes activités, puis le fichier de chacune (trace GPS, fréquence cardiaque, tours) pour les cartes et les records. Tu n'as qu'à suivre.",
                        ))
                        .child(self.sync_steps(cx))
                )
                .child(
                    section(12.)
                        .child(heading("Fond de carte"))
                        .child(div().text_color(t.soft).child(
                            "Par défaut, les tuiles d'OpenStreetMap, gardées sur le disque pour ne les demander qu'une fois. Tu peux mettre l'adresse d'un autre serveur (avec ta propre clé s'il en demande une).",
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(label("Adresse des tuiles ({z}, {x}, {y})"))
                                .child(Input::new(&self.tiles_input).cleanable(true)),
                        )
                        .child(
                            div().flex().gap(px(8.)).child(button("save-tiles", "Enregistrer", false, t).on_click(cx.listener(|this, _, window, cx| {
                                let v = this.tiles_input.read(cx).value().to_string();
                                this.save_tiles(v, window, cx);
                            }))),
                        ),
                )
                .child(
                    section(10.)
                        .child(heading("Où vont tes données"))
                        .child(fact(
                            IconName::Lock,
                            "La clé est confiée au trousseau du système (GNOME Keyring, KWallet, Trousseau macOS, Gestionnaire d'identifiants Windows) : chiffrée sur le disque, déverrouillée avec ta session. L'app ne l'écrit jamais dans un fichier.".into(),
                        ))
                        .child(fact(
                            IconName::Globe,
                            "Elle ne part que vers personal.connectapi.garmin.com, en HTTPS, échangée contre un jeton de 24 h gardé en mémoire. Les tuiles de carte sont demandées sans rien qui t'identifie.".into(),
                        ))
                        .child(fact(
                            IconName::HardDrive,
                            {
                                let dir = garmin::data_dir().display().to_string();
                                let home = std::env::var("HOME").unwrap_or_default();
                                let dir = if !home.is_empty() && dir.starts_with(&home) { dir.replacen(&home, "~", 1) } else { dir };
                                format!("Tes activités et leurs traces restent sur ta machine, dans {dir}, lisibles par toi seul.")
                            },
                        )),
                ),
        )
    }

    /// Where the track downloads stand, live: count, bar, the activity in progress, failures, and
    /// the button to resume or stop.
    pub fn tracks_progress(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let f = &self.fetch;
        let (read, total) = (self.derived.len().min(self.acts.len()), self.acts.len());
        let share = if total > 0 { read as f32 / total as f32 } else { 0.0 };
        let g = |n: usize| crate::activity::group(n as i64);
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(div().font_weight(FontWeight::BOLD).child(format!("{} / {} traces lues", g(read), g(total))))
                    .child(crate::ui::num(format!("{:.0} %", share * 100.0)).text_color(t.soft)),
            )
            .child(div().h(px(6.)).w_full().rounded(px(3.)).bg(t.border).child(div().h_full().rounded(px(3.)).bg(t.accent).w(relative(share))))
            .when_some(f.current.as_ref().filter(|_| f.running), |d, name| {
                d.child(div().text_size(px(12.)).text_color(t.soft).child(format!(
                    "Téléchargement {} / {} de cette série : « {name} »",
                    g((f.done + 1).min(f.total)),
                    g(f.total)
                )))
            })
            .when(!f.running && read < total && f.stopped.is_none() && self.key.is_some(), |d| {
                d.child(div().text_size(px(12.)).text_color(t.soft).child(format!("{} encore à télécharger.", g(total - read))))
            })
            .when(self.key.is_none() && read < total, |d| {
                d.child(div().text_size(px(12.)).text_color(t.soft).child("Ajoute ta clé API pour télécharger les traces."))
            })
            .when(f.failed > 0, |d| d.child(div().text_size(px(12.)).text_color(t.soft).child(format!("{} échec{} dans cette série", f.failed, if f.failed > 1 { "s" } else { "" }))))
            .when_some(f.error.clone(), |d, e| d.child(div().text_size(px(12.)).text_color(t.bad()).child(e)))
            .when_some(f.stopped.clone(), |d, e| d.child(div().text_size(px(12.)).text_color(t.bad()).child(e)))
            .child(div().flex().gap(px(8.)).when(f.running, |d| {
                d.child(button("fetch-stop", if f.cancel { "Arrêt après ce fichier…" } else { "Arrêter" }, false, t).on_click(cx.listener(|this, _, _, cx| {
                    this.fetch.cancel = true;
                    cx.notify();
                })))
            }).when(!f.running && read < total && self.key.is_some(), |d| {
                d.child(button("fetch-go", "Reprendre le téléchargement", true, t).on_click(cx.listener(|this, _, _, cx| {
                    this.fetch_tracks(cx);
                })))
            }))
    }

    /// Both steps of the synchronisation, live, with the button to start one.
    pub fn sync_steps(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let listing = matches!(&self.status, crate::app::Status::Busy(s) if s.starts_with("Synchronisation"));
        let step = |n: &'static str, label: &'static str, state: String, on: bool| {
            div()
                .flex()
                .gap(px(10.))
                .child(crate::ui::num(n).text_color(if on { t.accent } else { t.dim }))
                .child(div().w(px(150.)).flex_none().font_weight(if on { FontWeight::BOLD } else { FontWeight::NORMAL }).child(label))
                .child(div().flex_1().min_w_0().text_color(t.soft).child(state))
        };
        let list_state = if listing {
            match &self.status {
                crate::app::Status::Busy(s) => s.trim_start_matches("Synchronisation : ").to_string(),
                _ => String::new(),
            }
        } else {
            self.note.clone().unwrap_or_else(|| "pas encore lancée".into())
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(step("1", "Activités", list_state, listing))
            .child(step("2", "Fichiers", if self.fetch.running { "en cours".into() } else { String::new() }, self.fetch.running))
            .child(self.tracks_progress(cx))
            .when(!self.syncing() && self.key.is_some(), |d| {
                d.child(div().flex().child(button("sync-now", "Synchroniser maintenant", false, t).on_click(cx.listener(|this, _, _, cx| this.sync(cx)))))
            })
    }
}
