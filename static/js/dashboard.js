// Dashboard JavaScript for M3U Proxy Analytics

class Dashboard {
    constructor() {
        this.init();
    }

    init() {
        this.animateCounters();
        this.startLiveUpdates();
    }

    // Animate metric counters on page load
    animateCounters() {
        const counters = document.querySelectorAll('.metric-value');
        
        counters.forEach(counter => {
            const target = parseInt(counter.textContent.replace(/,/g, ''));
            const duration = 2000; // 2 seconds
            const steps = 60;
            const increment = target / steps;
            let current = 0;

            const timer = setInterval(() => {
                current += increment;
                if (current >= target) {
                    current = target;
                    clearInterval(timer);
                }
                
                // Format number with commas
                counter.textContent = Math.floor(current).toLocaleString();
            }, duration / steps);
        });
    }

    // Simulate live updates (this would be replaced with real API calls)
    startLiveUpdates() {
        // Update active streams every 30 seconds
        setInterval(() => {
            this.updateActiveStreams();
        }, 30000);

        // Update active clients every 15 seconds
        setInterval(() => {
            this.updateActiveClients();
        }, 15000);

        // Add new activity every 2-5 minutes
        setInterval(() => {
            this.addRandomActivity();
        }, Math.random() * 180000 + 120000); // 2-5 minutes
    }

    updateActiveStreams() {
        const streamsElement = document.getElementById('activeStreams');
        if (streamsElement) {
            const current = parseInt(streamsElement.textContent);
            const change = Math.floor(Math.random() * 10) - 5; // -5 to +5
            const newValue = Math.max(0, current + change);
            streamsElement.textContent = newValue.toLocaleString();
        }
    }

    updateActiveClients() {
        const clientsElement = document.getElementById('activeClients');
        if (clientsElement) {
            const current = parseInt(clientsElement.textContent);
            const change = Math.floor(Math.random() * 6) - 3; // -3 to +3
            const newValue = Math.max(0, current + change);
            clientsElement.textContent = newValue.toLocaleString();
        }
    }

    addRandomActivity() {
        const activities = [
            "New client connected from 192.168.1.{ip}",
            "Proxy \"Entertainment Mix\" regenerated ({channels} channels)",
            "Source \"Provider {provider}\" ingestion completed",
            "Data mapping rule \"Auto Clean\" applied",
            "Filter \"Sports Only\" updated",
            "Client disconnected from 10.0.0.{ip}",
            "Proxy \"News Channels\" accessed",
            "Logo cache updated ({count} logos)"
        ];

        const activityList = document.querySelector('.activity-list');
        if (!activityList) return;

        // Generate random activity
        const template = activities[Math.floor(Math.random() * activities.length)];
        const text = template
            .replace('{ip}', Math.floor(Math.random() * 254) + 1)
            .replace('{channels}', Math.floor(Math.random() * 500) + 100)
            .replace('{provider}', String.fromCharCode(65 + Math.floor(Math.random() * 26)))
            .replace('{count}', Math.floor(Math.random() * 50) + 10);

        // Create new activity item
        const newActivity = document.createElement('div');
        newActivity.className = 'activity-item';
        newActivity.innerHTML = `
            <span class="activity-time">just now</span>
            <span class="activity-text">${text}</span>
        `;

        // Add to top of list
        activityList.insertBefore(newActivity, activityList.firstChild);

        // Remove oldest activity if more than 10
        const activities_items = activityList.querySelectorAll('.activity-item');
        if (activities_items.length > 10) {
            activityList.removeChild(activities_items[activities_items.length - 1]);
        }

        // Update timestamps
        this.updateActivityTimestamps();
    }

    updateActivityTimestamps() {
        const timeElements = document.querySelectorAll('.activity-time');
        timeElements.forEach((element, index) => {
            if (index === 0) {
                element.textContent = 'just now';
            } else {
                const minutes = index * 2 + Math.floor(Math.random() * 5);
                element.textContent = `${minutes} min ago`;
            }
        });
    }

    // Method to show alerts (for future use)
    showAlert(type, message) {
        const alertsContainer = document.getElementById('alertsContainer');
        if (!alertsContainer) return;

        const alertId = 'alert-' + Date.now();
        const alertHtml = `
            <div id="${alertId}" class="alert alert-${type} alert-dismissible fade show" role="alert">
                ${message}
                <button type="button" class="btn-close" onclick="this.parentElement.remove()" aria-label="Close">&times;</button>
            </div>
        `;
        
        alertsContainer.insertAdjacentHTML('beforeend', alertHtml);
        
        // Auto-dismiss after 5 seconds
        setTimeout(() => {
            const alertElement = document.getElementById(alertId);
            if (alertElement) {
                alertElement.remove();
            }
        }, 5000);
    }
}

// Initialize dashboard when DOM is loaded
let dashboard;

function initializeDashboard() {
    console.log('Initializing dashboard...');
    dashboard = new Dashboard();
}

// Check if DOM is already loaded, if so initialize immediately
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initializeDashboard);
} else {
    initializeDashboard();
}