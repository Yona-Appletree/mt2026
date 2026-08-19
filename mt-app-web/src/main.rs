//! `mt-app-web`: the Dioxus web frontend. Talks to `mt-server` over `/api`
//! (proxied in dev, same-origin in production); owns the live pitch-trace
//! data plane (see AGENTS.md) but stays out of `mt-session`'s per-frame
//! loop. v0.1 stub: renders a placeholder.

use dioxus::prelude::*;

fn main() {
    dioxus::launch(app);
}

#[component]
fn app() -> Element {
    rsx! {
        div { "mt2026" }
    }
}
