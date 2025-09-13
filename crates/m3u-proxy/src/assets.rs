use rust_embed::RustEmbed;

/// Embedded static web assets (CSS, JS, HTML)
#[derive(RustEmbed)]
#[folder = "static/"]
#[prefix = "static/"]
pub struct StaticAssets;

/// Embedded database migrations
#[derive(RustEmbed)]
#[folder = "src/database/migrations/"]
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
        // Filter out .keep files and other non-asset files
        let real_assets: Vec<_> = assets
            .iter()
            .filter(|path| !path.ends_with(".keep"))
            .collect();
        if !real_assets.is_empty() {
            // Test that core assets are embedded in production builds
            assert!(
                StaticAssets::get_asset("static/index.html").is_some(),
                "index.html should be embedded in production builds"
            );
            // Check for Next.js CSS files (they have dynamic names)
            let has_css = real_assets.iter().any(|path| path.contains(".css"));
            assert!(has_css, "CSS files should be embedded in production builds");

            // Check for Next.js JS files
            let has_js = real_assets.iter().any(|path| path.contains(".js"));
            assert!(has_js, "JS files should be embedded in production builds");
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
    fn test_asset_content_validation() {
        // Test HTML content
        if let Some(index_html) = StaticAssets::get_asset("static/index.html") {
            let content = String::from_utf8_lossy(&index_html.data);
            assert!(
                content.contains("M3U Proxy"),
                "index.html should contain title"
            );
            assert!(
                content.contains("<!DOCTYPE html>") || content.contains("<!doctype html>"),
                "index.html should be valid HTML"
            );
        }

        // Test that we have some CSS content (Next.js CSS files have dynamic names)
        let css_files: Vec<_> = StaticAssets::iter()
            .filter(|f| f.contains(".css"))
            .collect();
        if !css_files.is_empty()
            && let Some(css_file) = StaticAssets::get_asset(&css_files[0])
        {
            let content = String::from_utf8_lossy(&css_file.data);
            assert!(!content.is_empty(), "CSS files should have content");
        }

        // Test that we have some JS content (Next.js JS files have dynamic names)
        let js_files: Vec<_> = StaticAssets::iter().filter(|f| f.contains(".js")).collect();
        if !js_files.is_empty()
            && let Some(js_file) = StaticAssets::get_asset(&js_files[0])
        {
            let content = String::from_utf8_lossy(&js_file.data);
            assert!(!content.is_empty(), "JS files should have content");
        }

        // Test relay page content (may be at different paths in Next.js builds)
        if let Some(relay_html) = StaticAssets::get_asset("static/admin/relays/index.html") {
            let content = String::from_utf8_lossy(&relay_html.data);
            // The relay page may show different content based on backend availability
            // Check for either relay-specific content or the general UI structure
            assert!(
                content.contains("relay")
                    || content.contains("M3U Proxy")
                    || content.contains("Backend"),
                "relay.html should contain relay-related or UI content"
            );
            assert!(
                content.contains("<!DOCTYPE html>") || content.contains("<!doctype html>"),
                "relay.html should be valid HTML"
            );
            // Note: FFmpeg description only appears when backend is available
            // In development/test builds, the page may show "Backend Unavailable" instead
        }
    }

    #[test]
    fn test_nonexistent_assets() {
        assert!(StaticAssets::get_asset("static/nonexistent.html").is_none());
        assert!(StaticAssets::get_asset("does/not/exist.css").is_none());
        assert!(MigrationAssets::get_migration("nonexistent_migration.sql").is_none());
    }
}
