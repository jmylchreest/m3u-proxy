use rust_embed::RustEmbed;

/// Embedded static web assets (CSS, JS, HTML)
#[derive(RustEmbed)]
#[folder = "static/"]
#[prefix = "static/"]
pub struct StaticAssets;

/// Embedded database migrations
#[derive(RustEmbed)]
#[folder = "migrations/"]
#[prefix = "migrations/"]
pub struct MigrationAssets;

impl StaticAssets {
    /// Get a static asset by path
    pub fn get_asset(path: &str) -> Option<rust_embed::EmbeddedFile> {
        Self::get(path)
    }

    /// Get the content type for a given file extension
    pub fn get_content_type(path: &str) -> &'static str {
        match path.split('.').next_back() {
            Some("html") => "text/html; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("js") => "application/javascript; charset=utf-8",
            Some("json") => "application/json; charset=utf-8",
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("svg") => "image/svg+xml; charset=utf-8",
            Some("ico") => "image/x-icon",
            Some("woff") => "font/woff",
            Some("woff2") => "font/woff2",
            Some("ttf") => "font/ttf",
            Some("eot") => "application/vnd.ms-fontobject",
            _ => "application/octet-stream",
        }
    }

    /// List all available static assets
    pub fn list_assets() -> impl Iterator<Item = std::borrow::Cow<'static, str>> {
        Self::iter()
    }
}

impl MigrationAssets {
    /// Get a migration file by path
    pub fn get_migration(path: &str) -> Option<rust_embed::EmbeddedFile> {
        Self::get(path)
    }

    /// Get all migration files in order
    pub fn get_migrations() -> Vec<(String, String)> {
        let mut migrations = Vec::new();

        for file_path in Self::iter() {
            if let Some(file) = Self::get(&file_path) {
                let content = String::from_utf8_lossy(&file.data).to_string();
                let name = file_path
                    .strip_prefix("migrations/")
                    .unwrap_or(&file_path)
                    .to_string();
                migrations.push((name, content));
            }
        }

        // Sort migrations by filename to ensure proper order
        migrations.sort_by(|a, b| a.0.cmp(&b.0));
        migrations
    }

    /// List all available migration files
    pub fn list_migrations() -> impl Iterator<Item = std::borrow::Cow<'static, str>> {
        Self::iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_type_detection() {
        assert_eq!(
            StaticAssets::get_content_type("test.html"),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            StaticAssets::get_content_type("test.css"),
            "text/css; charset=utf-8"
        );
        assert_eq!(
            StaticAssets::get_content_type("test.js"),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(
            StaticAssets::get_content_type("test.json"),
            "application/json; charset=utf-8"
        );
        assert_eq!(StaticAssets::get_content_type("test.png"), "image/png");
        assert_eq!(StaticAssets::get_content_type("test.jpg"), "image/jpeg");
        assert_eq!(StaticAssets::get_content_type("test.gif"), "image/gif");
        assert_eq!(
            StaticAssets::get_content_type("test.svg"),
            "image/svg+xml; charset=utf-8"
        );
        assert_eq!(StaticAssets::get_content_type("test.ico"), "image/x-icon");
        assert_eq!(StaticAssets::get_content_type("test.woff"), "font/woff");
        assert_eq!(StaticAssets::get_content_type("test.woff2"), "font/woff2");
        assert_eq!(
            StaticAssets::get_content_type("unknown"),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_static_assets_exist() {
        // Test that we can list assets (will be populated at compile time)
        let assets: Vec<_> = StaticAssets::list_assets().collect();
        // In a real build, this should contain our static files
        println!("Static assets found: {assets:?}");

        // Only test for assets if they are actually embedded (development vs production build)
        if !assets.is_empty() {
            // Test that core assets are embedded in production builds
            assert!(
                StaticAssets::get_asset("static/html/index.html").is_some(),
                "index.html should be embedded in production builds"
            );
            assert!(
                StaticAssets::get_asset("static/css/main.css").is_some(),
                "main.css should be embedded in production builds"
            );
            assert!(
                StaticAssets::get_asset("static/js/shared-loader.js").is_some(),
                "shared-loader.js should be embedded in production builds"
            );
        } else {
            // In development builds without assets, just verify the system works
            println!("Development build detected - no static assets embedded");
        }
    }

    #[test]
    fn test_migration_assets_exist() {
        // Test that we can list migrations (will be populated at compile time)
        let migrations: Vec<_> = MigrationAssets::list_migrations().collect();
        // In a real build, this should contain our migration files
        println!("Migration assets found: {migrations:?}");

        let migration_list = MigrationAssets::get_migrations();
        assert!(
            !migration_list.is_empty(),
            "Should have at least one migration"
        );

        // Verify migrations are sorted
        for i in 1..migration_list.len() {
            assert!(
                migration_list[i - 1].0 <= migration_list[i].0,
                "Migrations should be sorted by name"
            );
        }
    }

    #[test]
    fn test_shared_templates_exist() {
        // Test that shared templates are embedded (only in production builds)
        let assets: Vec<_> = StaticAssets::list_assets().collect();
        
        if !assets.is_empty() {
            // Only test for templates if assets are embedded
            assert!(
                StaticAssets::get_asset("static/html/shared/header.html").is_some(),
                "header.html should be embedded in production builds"
            );
            assert!(
                StaticAssets::get_asset("static/html/shared/nav.html").is_some(),
                "nav.html should be embedded in production builds"
            );
            assert!(
                StaticAssets::get_asset("static/html/shared/channels-modal.html").is_some(),
                "channels-modal.html should be embedded in production builds"
            );
            assert!(
                StaticAssets::get_asset("static/html/shared/footer-with-channels.html").is_some(),
                "footer-with-channels.html should be embedded in production builds"
            );
        } else {
            println!("Development build detected - no template assets embedded");
        }
    }

    #[test]
    fn test_asset_content_validation() {
        // Test HTML content
        if let Some(index_html) = StaticAssets::get_asset("static/html/index.html") {
            let content = String::from_utf8_lossy(&index_html.data);
            assert!(
                content.contains("M3U Proxy"),
                "index.html should contain title"
            );
            assert!(
                content.contains("<!doctype html>"),
                "index.html should be valid HTML"
            );
        }

        // Test CSS content
        if let Some(main_css) = StaticAssets::get_asset("static/css/main.css") {
            let content = String::from_utf8_lossy(&main_css.data);
            assert!(
                content.contains("--primary-color"),
                "main.css should contain CSS variables"
            );
        }

        // Test JS content
        if let Some(shared_loader_js) = StaticAssets::get_asset("static/js/shared-loader.js") {
            let content = String::from_utf8_lossy(&shared_loader_js.data);
            assert!(
                content.contains("TemplateLoader"),
                "shared-loader.js should contain TemplateLoader class"
            );
        }

        // Test relay.html content
        if let Some(relay_html) = StaticAssets::get_asset("static/html/relay.html") {
            let content = String::from_utf8_lossy(&relay_html.data);
            assert!(
                content.contains("Stream Relay"),
                "relay.html should contain Stream Relay title"
            );
            assert!(
                content.contains("<!doctype html>"),
                "relay.html should be valid HTML"
            );
            assert!(
                content.contains("uses FFmpeg"),
                "relay.html should contain FFmpeg description"
            );
        }
    }

    #[test]
    fn test_nonexistent_assets() {
        assert!(StaticAssets::get_asset("static/nonexistent.html").is_none());
        assert!(StaticAssets::get_asset("does/not/exist.css").is_none());
        assert!(MigrationAssets::get_migration("nonexistent_migration.sql").is_none());
    }
}
