use handlebars::{Context, Handlebars, Helper, HelperResult, Output, RenderContext};

pub struct ImageHelper;

impl handlebars::HelperDef for ImageHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let image_url = h
            .param(0)
            .and_then(|v| v.value().as_str())
            .ok_or_else(|| handlebars::RenderErrorReason::InvalidParamType("Expected image url"))?;

        out.write(
            format!(
                "<img src=\"{}\" alt=\"image\" class=\"w-15 h-15 object-contain\" />",
                image_url
            )
            .as_str(),
        )?;
        Ok(())
    }
}
