/// An implementation of [`poll_read/write_ready`] that forwards readiness checks to a field.
#[allow(unused_macros)]
macro_rules! forward_ready {
    ($field:ident) => {
        #[inline]
        fn poll_read_ready(&self, cx: &mut std::task::Context<'_>) -> Poll<$crate::io::Readiness> {
            self.$field.poll_read_ready(cx)
        }

        #[inline]
        fn poll_write_ready(&self, cx: &mut std::task::Context<'_>) -> Poll<$crate::io::Readiness> {
            self.$field.poll_write_ready(cx)
        }
    };
}

#[allow(unused_macros)]
macro_rules! forward_query {
    ($field:ident) => {
        #[inline]
        fn query(&self, id: std::any::TypeId) -> Option<Box<dyn std::any::Any>> {
            self.$field.query(id)
        }
    };
}

#[allow(unused_macros)]
macro_rules! forward_shutdown {
    ($field:ident) => {
        #[inline]
        fn shutdown(
            &self,
            ctx: &mut $crate::io::FilterCtx<'_>,
        ) -> std::io::Result<std::task::Poll<()>> {
            self.$field.shutdown(ctx)
        }
    };
}

#[allow(unused_imports)]
pub(crate) use {forward_query, forward_ready, forward_shutdown};
