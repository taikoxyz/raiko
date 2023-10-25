macro_rules! cfg_taiko {
  ($($item:item)*) => {
      $(
          #[cfg(feature = "taiko")]
          #[cfg_attr(docsrs, doc(cfg(feature = "taiko")))]
          $item
      )*
  }
}
