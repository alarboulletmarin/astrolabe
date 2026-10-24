//! Settings: the Garmin key, the map tiles, and where the data goes. A centred column, like a note.

use crate::data::{Data, Status};
use crate::platform::secrets::Store;
use crate::platform::{store, tiles};
use crate::ui::{button, heading, icon, pill, title};
use crate::views::{Width, tracks_progress};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{prelude::FluentBuilder, *};

pub struct SettingsView {
    data: Entity<Data>,
    width: Width,
    key_input: Entity<InputState>,
    tiles_input: Entity<InputState>,
    _subscription: Subscription,
}

impl SettingsView {
    pub fn new(data: Entity<Data>, width: Width, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let key_input = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("GCDC_…"));
        let saved = data.read(cx).settings.get("tiles").cloned().unwrap_or_default();
        let tiles_input = cx.new(|cx| InputState::new(window, cx).placeholder(tiles::DEFAULT_TILES).default_value(saved));
        let sub = cx.observe(&data, |_, _, cx| cx.notify());
        SettingsView { data, width, key_input, tiles_input, _subscription: sub }
    }

    fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.key_input.read(cx).value().to_string();
        if self.data.update(cx, |d, cx| d.save_key(&raw, cx)) {
            self.key_input.update(cx, |s, cx| s.set_value("", window, cx));
        }
    }

    fn save_tiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.tiles_input.read(cx).value().to_string();
        if self.data.update(cx, |d, cx| d.save_tiles(&value, cx)) && value.trim().is_empty() {
            self.tiles_input.update(cx, |s, cx| s.set_value("", window, cx));
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.get(window);
        let d = self.data.read(cx);
        let t = d.theme;
        let has = d.key.is_some();
        let no_store = if let Store::Vault(why) = &d.key_store { Some(why.clone()) } else { None };
        let checking = d.busy() && d.key_error.is_none();
        let key_error = d.key_error.clone();
        let fact = |name: IconName, text: String| {
            div()
                .flex()
                .gap(px(10.))
                .child(icon(name, t.soft, 14.).flex_none().mt(px(3.)))
                .child(div().flex_1().min_w_0().text_color(t.soft).child(text))
        };
        let label = |text: &'static str| div().text_size(px(12.)).text_color(t.soft).child(text);
        let section = |gap: f32| div().flex().flex_col().gap(px(gap));

        div().id("settings").size_full().overflow_y_scroll().track_scroll(self.width.scroll()).child(
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
                            pill(if no_store.is_some() { "chiffrée dans l'app" } else { "dans le trousseau" }, t.good())
                        } else {
                            pill("aucune", (t.panel, t.soft))
                        }))
                        .when_some(no_store.clone(), |d, why| {
                            d.child(div().text_color(t.soft).child(format!("{why} La clé est donc chiffrée par l'app, dans un fichier que toi seul peux lire.")))
                        })
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
                                .child(match &key_error {
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
                                .when(has, |d| d.child(button("delete-key", "Retirer la clé", false, t).on_click(cx.listener(|this, _, _, cx| this.data.update(cx, |d, cx| d.delete_key(cx)))))),
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
                            div().flex().gap(px(8.)).child(button("save-tiles", "Enregistrer", false, t).on_click(cx.listener(|this, _, window, cx| this.save_tiles(window, cx)))),
                        ),
                )
                .child(
                    section(10.)
                        .child(heading("Où vont tes données"))
                        .child(fact(
                            IconName::Lock,
                            if no_store.is_some() {
                                "Faute de trousseau système, la clé est chiffrée (XChaCha20-Poly1305) dans un fichier de l'app, et la clé qui l'ouvre est gardée à part, dans ton dossier de configuration. L'une sans l'autre ne sert à rien ; l'app ne l'écrit jamais en clair.".into()
                            } else {
                                "La clé est confiée au trousseau du système (GNOME Keyring, KWallet, Trousseau macOS, Gestionnaire d'identifiants Windows) : chiffrée sur le disque, déverrouillée avec ta session. L'app ne l'écrit jamais dans un fichier.".into()
                            },
                        ))
                        .child(fact(
                            IconName::Globe,
                            "Elle ne part que vers personal.connectapi.garmin.com, en HTTPS, échangée contre un jeton de 24 h gardé en mémoire. Les tuiles de carte sont demandées sans rien qui t'identifie.".into(),
                        ))
                        .child(fact(
                            IconName::HardDrive,
                            {
                                let dir = store::data_dir().display().to_string();
                                let home = std::env::var("HOME").unwrap_or_default();
                                let dir = if !home.is_empty() && dir.starts_with(&home) { dir.replacen(&home, "~", 1) } else { dir };
                                format!("Tes activités et leurs traces restent sur ta machine, dans {dir}, lisibles par toi seul.")
                            },
                        )),
                ),
        )
    }

}

impl SettingsView {
    /// Both steps of the synchronisation, live, with the button to start one.
    fn sync_steps(&self, cx: &mut Context<Self>) -> Div {
        let d = self.data.read(cx);
        let t = d.theme;
        let listing = d.listing();
        let step = |n: &'static str, label: &'static str, state: String, on: bool| {
            div()
                .flex()
                .gap(px(10.))
                .child(crate::ui::num(n).text_color(if on { t.accent } else { t.dim }))
                .child(div().w(px(150.)).flex_none().font_weight(if on { FontWeight::BOLD } else { FontWeight::NORMAL }).child(label))
                .child(div().flex_1().min_w_0().text_color(t.soft).child(state))
        };
        let list_state = if listing {
            match &d.status {
                Status::Busy(s) => s.trim_start_matches("Synchronisation : ").to_string(),
                _ => String::new(),
            }
        } else {
            d.note.clone().unwrap_or_else(|| "pas encore lancée".into())
        };
        let (fetching, syncing, has_key) = (d.fetch.running, d.syncing(), d.key.is_some());
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(step("1", "Activités", list_state, listing))
            .child(step("2", "Fichiers", if fetching { "en cours".into() } else { String::new() }, fetching))
            .child(tracks_progress(&self.data, cx))
            .when(!syncing && has_key, |d| {
                d.child(div().flex().child(button("sync-now", "Synchroniser maintenant", false, t).on_click(cx.listener(|this, _, _, cx| this.data.update(cx, |d, cx| d.sync(cx))))))
            })
    }
}
