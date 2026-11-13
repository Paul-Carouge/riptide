//! Cached slices of the main window.
//!
//! GPUI re-renders, lays out and paints a whole view whenever it is notified. A `Region` is a
//! small view that draws one slice of `RiptideApp` (still owner of all state) and is embedded
//! with `AnyView::cached`, so a frame reuses its previous layout and paint unless the region
//! itself was notified. The app notifies every region on any state change (see the observer
//! below); the animation ticker notifies only the regions that move. A spinning disc therefore
//! repaints its own queue card, not the paper grain, the left column and the other cards.

use gpui::{
    AnyElement, App, AppContext, Context, Entity, IntoElement, Render, StyleRefinement,
    Subscription, WeakEntity, Window, div,
};

use super::app::RiptideApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionKind {
    PaperGrain,
    FaceA,
    ToolsPulse,
    QueueCard(u64),
}

pub struct Region {
    app: WeakEntity<RiptideApp>,
    kind: RegionKind,
    _app_observer: Subscription,
}

impl Region {
    pub fn new(app: &Entity<RiptideApp>, kind: RegionKind, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            app: app.downgrade(),
            kind,
            // Any app state change may show up in any region.
            _app_observer: cx.observe(app, |_, _, cx| cx.notify()),
        })
    }
}

impl Render for Region {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.kind;
        self.app
            .update(cx, |app, cx| app.render_region(kind, window, cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

/// The region as a cached element. `layout` must size it without its content (fixed or
/// flex size): a cached view is laid out from this style alone.
pub fn cached(region: &Entity<Region>, layout: StyleRefinement) -> AnyElement {
    gpui::AnyView::from(region.clone())
        .cached(layout)
        .into_any_element()
}
