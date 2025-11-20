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
        let value = h
            .param(0)
            .and_then(|v| v.value().as_str())
            .ok_or_else(|| handlebars::RenderErrorReason::InvalidParamType("Expected string param"))?;

        if value.starts_with("http") || value.starts_with("data:image") {
            out.write(
                format!(
                    "<img src=\"{}\" alt=\"image\" style=\"width: 48px; height: 48px; object-fit: contain;\" />",
                    value
                )
                .as_str(),
            )?;
        } else {
            out.write(
                format!(
                    "<span style=\"font-size: 20px; font-weight: 700; color: #000000;\">{}</span>",
                    handlebars::html_escape(value)
                )
                .as_str(),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use handlebars::Handlebars;

    #[test]
    fn test_image_helper_url() {
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("image", Box::new(ImageHelper));

        let result = handlebars
            .render_template("{{image url}}", &serde_json::json!({"url": "https://example.com/logo.png"}))
            .unwrap();
        assert_eq!(result, "<img src=\"https://example.com/logo.png\" alt=\"image\" style=\"width: 60px; height: 60px; object-fit: contain;\" />");
    }

    #[test]
    fn test_image_helper_base64() {
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("image", Box::new(ImageHelper));

        let result = handlebars
            .render_template("{{image url}}", &serde_json::json!({"url": "data:image/png;base64,abc"}))
            .unwrap();
        assert_eq!(result, "<img src=\"data:image/png;base64,abc\" alt=\"image\" style=\"width: 60px; height: 60px; object-fit: contain;\" />");
    }

    #[test]
    fn test_image_helper_text() {
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("image", Box::new(ImageHelper));

        let result = handlebars
            .render_template("{{image name}}", &serde_json::json!({"name": "MyApp"}))
            .unwrap();
        assert_eq!(result, "<span style=\"font-size: 20px; font-weight: 700; color: #000000;\">MyApp</span>");
    }
}
