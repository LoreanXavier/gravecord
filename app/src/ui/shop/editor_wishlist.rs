//! What the profile editor's Wishlist tab borrows from the Shop: the product behind a sku, its preview art and the
//! request for products no answer has delivered yet.

use super::Root;
use gpui::{AnyElement, Context, Window};

impl Root {
    /// Asks for the products of these skus (`requestProducts`), once each.
    pub(in crate::ui) fn editor_wishlist_need(&mut self, skus: &[String]) {
        self.shop_need_products(skus);
    }

    /// The square art of a sku's product (the card preview of the Shop at `size`), or `None` while it loads.
    pub(in crate::ui) fn editor_wishlist_preview(
        &mut self,
        sku: &str,
        size: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let product = self.shop_product(sku)?;
        Some(self.shop_preview(&product, size, false, false, window, cx))
    }
}
