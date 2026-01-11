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
        let url = h.param(0).and_then(|v| v.value().as_str());
        let alt_name = h
            .param(1)
            .and_then(|v| v.value().as_str())
            .unwrap_or("Logo");

        match url {
            Some(src) if !src.is_empty() => {
                out.write(&format!(
                    "<img src=\"{}\" alt=\"{} Logo\" style=\"max-height:60px; width:auto; object-fit: contain;\" />",
                    src, alt_name
                ))?;
            }
            _ => {
                out.write(&format!(
                    "<span style=\"font-size: 20px; font-weight: 700; color: #000000;\">{}</span>",
                    alt_name
                ))?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use handlebars::Handlebars;

    #[test]
    fn test_image_helper_with_url_and_alt() {
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("image", Box::new(ImageHelper));

        let result = handlebars
            .render_template(
                "{{image app.logo app.name}}",
                &serde_json::json!({"app": {"logo": "https://example.com/logo.png", "name": "MyApp"}}),
            )
            .unwrap();
        assert_eq!(
            result,
            "<img src=\"https://example.com/logo.png\" alt=\"MyApp Logo\" style=\"max-height:60px; width:auto; object-fit: contain;\" />"
        );
    }

    #[test]
    fn test_image_helper_no_url_fallback_to_text() {
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("image", Box::new(ImageHelper));

        let result = handlebars
            .render_template(
                "{{image app.logo app.name}}",
                &serde_json::json!({"app": {"logo": null, "name": "MyApp"}}),
            )
            .unwrap();
        assert_eq!(
            result,
            "<span style=\"font-size: 20px; font-weight: 700; color: #000000;\">MyApp</span>"
        );
    }

    #[test]
    fn test_image_helper_empty_url_fallback_to_text() {
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("image", Box::new(ImageHelper));

        let result = handlebars
            .render_template(
                "{{image app.logo app.name}}",
                &serde_json::json!({"app": {"logo": "", "name": "MyApp"}}),
            )
            .unwrap();
        assert_eq!(
            result,
            "<span style=\"font-size: 20px; font-weight: 700; color: #000000;\">MyApp</span>"
        );
    }
}
