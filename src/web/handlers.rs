use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, Response},
};

use super::AppState;

pub async fn serve_proxy_m3u(
    Path(_ulid): Path<String>,
    State(_state): State<AppState>,
) -> Result<Response<String>, StatusCode> {
    // TODO: Implement M3U serving logic
    Ok(Response::builder()
        .header("content-type", "application/vnd.apple.mpegurl")
        .body(format!("#EXTM3U\n# Proxy ULID: {}\n", _ulid))
        .unwrap())
}

pub async fn serve_logo(
    Path(_logo_id): Path<String>,
    State(_state): State<AppState>,
) -> StatusCode {
    // TODO: Implement logo serving logic
    StatusCode::NOT_FOUND
}

pub async fn index() -> Html<&'static str> {
    Html(
        r#"
<!DOCTYPE html>
<html>
<head>
    <title>M3U Proxy</title>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; }
        .container { max-width: 1200px; margin: 0 auto; }
        nav { background: #f8f9fa; padding: 1rem; border-radius: 8px; margin-bottom: 2rem; }
        nav a { margin-right: 1rem; text-decoration: none; color: #007bff; }
        .card { background: white; border: 1px solid #dee2e6; border-radius: 8px; padding: 1.5rem; margin-bottom: 1rem; }
        h1 { color: #343a40; }
    </style>
</head>
<body>
    <div class="container">
        <h1>M3U Proxy Service</h1>
        <nav>
            <a href="/">Home</a>
            <a href="/sources">Stream Sources</a>
            <a href="/proxies">Stream Proxies</a>
            <a href="/filters">Filters</a>
        </nav>
        <div class="card">
            <h2>Welcome to M3U Proxy</h2>
            <p>A modern M3U proxy service with filtering and source management.</p>
            <p>Use the navigation above to manage your stream sources, proxies, and filters.</p>
        </div>
    </div>
</body>
</html>
    "#,
    )
}

pub async fn sources_page() -> Html<&'static str> {
    Html(
        r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Stream Sources - M3U Proxy</title>
    <link rel="stylesheet" href="/static/css/main.css">
</head>
<body>
    <header>
        <div class="container">
            <div class="header-content">
                <a href="/" class="logo">M3U Proxy</a>
            </div>
        </div>
    </header>

    <div class="container">
        <nav>
            <ul>
                <li><a href="/">Home</a></li>
                <li><a href="/sources" class="active">Stream Sources</a></li>
                <li><a href="/proxies">Stream Proxies</a></li>
                <li><a href="/filters">Filters</a></li>
            </ul>
        </nav>

        <div id="alertsContainer"></div>

        <div class="card">
            <div class="card-header">
                <h2 class="card-title">Stream Sources</h2>
                <button id="addSourceBtn" class="btn btn-primary">Add Source</button>
            </div>

            <div id="loadingIndicator" class="text-center" style="display: none;">
                <span class="loading"></span> Loading sources...
            </div>

            <div class="table-responsive">
                <table id="sourcesTable" class="table">
                    <thead>
                        <tr>
                            <th>Name / URL</th>
                            <th>Type</th>
                            <th>Max Streams</th>
                            <th>Channels</th>
                            <th>Status</th>
                            <th>Last Updated</th>
                            <th>Actions</th>
                        </tr>
                    </thead>
                    <tbody id="sourcesTableBody">
                        <!-- Sources will be loaded here -->
                    </tbody>
                </table>
            </div>
        </div>
    </div>

    <!-- Source Modal -->
    <div id="sourceModal" class="modal">
        <div class="modal-content">
            <div class="modal-header">
                <h3 id="modalTitle" class="modal-title">Add Stream Source</h3>
                <button id="closeModal" class="modal-close">&times;</button>
            </div>
            <div class="modal-body">
                <form id="sourceForm">
                    <div class="form-group">
                        <label for="sourceName" class="form-label">Source Name *</label>
                        <input type="text" id="sourceName" class="form-control" required>
                    </div>

                    <div class="form-group">
                        <label for="sourceType" class="form-label">Source Type *</label>
                        <select id="sourceType" class="form-select" required>
                            <option value="m3u">M3U Playlist</option>
                            <option value="xtream">Xtream Codes</option>
                        </select>
                    </div>

                    <div class="form-group">
                        <label for="sourceUrl" class="form-label">Source URL *</label>
                        <input type="url" id="sourceUrl" class="form-control" required>
                    </div>

                    <div class="form-group">
                        <label for="maxStreams" class="form-label">Max Concurrent Streams</label>
                        <input type="number" id="maxStreams" class="form-control" min="1" value="1">
                    </div>

                    <div class="form-group">
                        <label for="updateCron" class="form-label">Update Schedule (Cron)</label>
                        <input type="text" id="updateCron" class="form-control" value="0 */6 * * *" placeholder="0 */6 * * *">
                        <small class="text-muted">Default: Every 6 hours</small>
                    </div>

                    <!-- Xtream Codes Fields -->
                    <div id="xtreamFields" style="display: none;">
                        <div class="form-group">
                            <label for="username" class="form-label">Username *</label>
                            <input type="text" id="username" class="form-control">
                        </div>

                        <div class="form-group">
                            <label for="password" class="form-label">Password *</label>
                            <div class="password-field">
                                <input type="password" id="password" class="form-control password-input">
                                <button type="button" class="password-toggle" onclick="togglePassword('password')"></button>
                            </div>
                        </div>
                    </div>

                    <!-- M3U Fields -->
                    <div id="m3uFields">
                        <div class="form-group">
                            <label for="fieldMap" class="form-label">Field Mapping (JSON)</label>
                            <textarea id="fieldMap" class="form-control" rows="3" placeholder='{"group_field": "group-title", "logo_field": "tvg-logo"}'></textarea>
                            <small class="text-muted">Optional: Custom field mapping for M3U parsing</small>
                        </div>
                    </div>

                    <div class="form-group">
                        <div class="form-check">
                            <input type="checkbox" id="isActive" class="form-check-input" checked>
                            <label for="isActive" class="form-label">Active</label>
                        </div>
                    </div>
                </form>
            </div>
            <div class="modal-footer">
                <button id="cancelSource" class="btn btn-secondary">Cancel</button>
                <button id="saveSource" class="btn btn-primary">Save</button>
            </div>
        </div>
    </div>

    <!-- Channels Modal -->
    <div id="channelsModal" class="modal channels-modal">
        <div class="modal-content channels-modal-content">
            <div class="modal-header">
                <h3 id="channelsModalTitle" class="modal-title">Channels</h3>
                <button onclick="sourcesManager.hideChannelsModal()" class="modal-close" title="Close" aria-label="Close channels modal">✕</button>
            </div>
            <div class="modal-body">
                <div id="channelsLoading" class="text-center" style="display: none;">
                    <span class="loading"></span> Loading channels...
                </div>

                <div id="channelsContent" style="display: none;">
                    <div class="mb-3">
                        <input
                            type="text"
                            id="channelsFilter"
                            class="form-control"
                            placeholder="Filter channels by name, group, or TVG name..."
                            oninput="sourcesManager.filterChannels()"
                        >
                    </div>

                    <div class="mb-3">
                        <span id="channelsCount" class="text-muted">0 channels</span>
                    </div>

                    <div class="table-responsive channels-table-container">
                        <table class="table table-striped">
                            <thead>
                                <tr>
                                    <th>Channel Name</th>
                                    <th>Group</th>
                                    <th>TVG ID</th>
                                    <th>Logo</th>
                                </tr>
                            </thead>
                            <tbody id="channelsTableBody">
                                <!-- Channels will be loaded here -->
                            </tbody>
                        </table>
                    </div>
                </div>
            </div>
            <div class="modal-footer">
                <button onclick="sourcesManager.hideChannelsModal()" class="btn btn-secondary">Close</button>
            </div>
        </div>
    </div>

    <script src="/static/js/sources.js"></script>
</body>
</html>
    "#,
    )
}

pub async fn proxies_page() -> Html<&'static str> {
    Html(
        r#"
<!DOCTYPE html>
<html>
<head>
    <title>Stream Proxies - M3U Proxy</title>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; }
        .container { max-width: 1200px; margin: 0 auto; }
        nav { background: #f8f9fa; padding: 1rem; border-radius: 8px; margin-bottom: 2rem; }
        nav a { margin-right: 1rem; text-decoration: none; color: #007bff; }
        .card { background: white; border: 1px solid #dee2e6; border-radius: 8px; padding: 1.5rem; margin-bottom: 1rem; }
        h1 { color: #343a40; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Stream Proxies</h1>
        <nav>
            <a href="/">Home</a>
            <a href="/sources">Stream Sources</a>
            <a href="/proxies">Stream Proxies</a>
            <a href="/filters">Filters</a>
        </nav>
        <div class="card">
            <h2>Manage Stream Proxies</h2>
            <p>Stream proxies management interface will be implemented here.</p>
        </div>
    </div>
</body>
</html>
    "#,
    )
}

pub async fn filters_page() -> Html<&'static str> {
    Html(
        r#"
<!DOCTYPE html>
<html>
<head>
    <title>Filters - M3U Proxy</title>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; }
        .container { max-width: 1200px; margin: 0 auto; }
        nav { background: #f8f9fa; padding: 1rem; border-radius: 8px; margin-bottom: 2rem; }
        nav a { margin-right: 1rem; text-decoration: none; color: #007bff; }
        .card { background: white; border: 1px solid #dee2e6; border-radius: 8px; padding: 1.5rem; margin-bottom: 1rem; }
        h1 { color: #343a40; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Filters</h1>
        <nav>
            <a href="/">Home</a>
            <a href="/sources">Stream Sources</a>
            <a href="/proxies">Stream Proxies</a>
            <a href="/filters">Filters</a>
        </nav>
        <div class="card">
            <h2>Manage Filters</h2>
            <p>Filters management interface will be implemented here.</p>
        </div>
    </div>
</body>
</html>
    "#,
    )
}
