// EPG Viewer JavaScript

class EpgViewer {
    constructor() {
        this.epgData = null;
        this.filteredChannels = [];
        this.currentFilter = '';
        this.currentDate = new Date();
        this.currentStartTime = new Date();
        this.timeRangeHours = 12;
        this.tooltip = null;
        this.init();
    }

    async init() {
        this.setupEventListeners();
        this.initializeDateTime();
        this.tooltip = document.getElementById('epgTooltip');
        await this.loadEpgData();
    }

    setupEventListeners() {
        // Channel filter
        document.getElementById('channelFilter').addEventListener('input', (e) => {
            this.currentFilter = e.target.value.toLowerCase();
            this.filterAndRenderChannels();
        });

        // Date selector
        document.getElementById('dateSelect').addEventListener('change', (e) => {
            this.currentDate = new Date(e.target.value + 'T00:00:00');
            this.updateStartTime();
            this.loadEpgData();
        });

        // Time range selector
        document.getElementById('timeRange').addEventListener('change', (e) => {
            this.timeRangeHours = parseInt(e.target.value);
            this.loadEpgData();
        });

        // Start time selector
        document.getElementById('startTime').addEventListener('change', (e) => {
            const [hours, minutes] = e.target.value.split(':');
            this.currentStartTime = new Date(this.currentDate);
            this.currentStartTime.setHours(parseInt(hours), parseInt(minutes), 0, 0);
            this.loadEpgData();
        });

        // Refresh button
        document.getElementById('refreshEpgBtn').addEventListener('click', () => {
            this.loadEpgData();
        });

        // Now button
        document.getElementById('nowBtn').addEventListener('click', () => {
            this.jumpToNow();
        });

        // Tooltip events
        document.addEventListener('mouseleave', () => {
            this.hideTooltip();
        });
    }

    initializeDateTime() {
        const now = new Date();

        // Set current date
        const dateStr = now.toISOString().split('T')[0];
        document.getElementById('dateSelect').value = dateStr;
        this.currentDate = new Date(dateStr + 'T00:00:00');

        // Set current hour as start time
        const currentHour = now.getHours();
        const timeStr = String(currentHour).padStart(2, '0') + ':00';
        document.getElementById('startTime').value = timeStr;

        this.currentStartTime = new Date(this.currentDate);
        this.currentStartTime.setHours(currentHour, 0, 0, 0);
    }

    updateStartTime() {
        const startTimeInput = document.getElementById('startTime');
        const [hours, minutes] = startTimeInput.value.split(':');
        this.currentStartTime = new Date(this.currentDate);
        this.currentStartTime.setHours(parseInt(hours), parseInt(minutes), 0, 0);
    }

    async loadEpgData() {
        try {
            this.showLoading();

            const endTime = new Date(this.currentStartTime);
            endTime.setHours(endTime.getHours() + this.timeRangeHours);

            const params = new URLSearchParams({
                start_time: this.currentStartTime.toISOString(),
                end_time: endTime.toISOString()
            });

            // Note: We do client-side filtering instead of server-side for better flexibility

            const response = await fetch(`/api/v1/epg/viewer?${params}`);

            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }

            this.epgData = await response.json();
            this.filteredChannels = [...this.epgData.channels];

            this.renderEpgGrid();
            this.hideLoading();

        } catch (error) {
            console.error('Error loading EPG data:', error);
            this.showError('Failed to load EPG data: ' + error.message);
            this.hideLoading();
        }
    }

    renderEpgGrid() {
        if (!this.epgData || this.epgData.channels.length === 0) {
            this.showNoData();
            return;
        }

        this.showContent();
        this.renderTimeline();
        this.filterAndRenderChannels();
    }

    renderTimeline() {
        const timeline = document.getElementById('epgTimeline');
        timeline.innerHTML = '';

        // Add channel info spacer
        const spacer = document.createElement('div');
        spacer.style.minWidth = '200px';
        spacer.style.borderRight = '1px solid #dee2e6';
        timeline.appendChild(spacer);

        // Generate time slots
        const startTime = new Date(this.currentStartTime);
        const endTime = new Date(startTime);
        endTime.setHours(endTime.getHours() + this.timeRangeHours);

        const currentTime = startTime;
        while (currentTime < endTime) {
            const timeSlot = document.createElement('div');
            timeSlot.className = 'epg-time-slot';
            timeSlot.textContent = this.formatTime(currentTime);
            timeline.appendChild(timeSlot);

            currentTime.setMinutes(currentTime.getMinutes() + 30); // 30-minute slots
        }
    }

    filterAndRenderChannels() {
        if (!this.epgData) return;

        // Filter channels
        this.filteredChannels = this.epgData.channels.filter(channel => {
            if (!this.currentFilter) return true;
            return channel.channel.channel_name.toLowerCase().includes(this.currentFilter) ||
                   channel.channel.channel_id.toLowerCase().includes(this.currentFilter);
        });

        // Update channel count
        document.getElementById('channelCount').textContent = `${this.filteredChannels.length} channels`;

        // Render channels
        this.renderChannels();
    }

    renderChannels() {
        const channelsContainer = document.getElementById('epgChannels');
        channelsContainer.innerHTML = '';

        if (this.filteredChannels.length === 0) {
            const noChannels = document.createElement('div');
            noChannels.className = 'text-center text-muted p-4';
            noChannels.textContent = 'No channels match your filter';
            channelsContainer.appendChild(noChannels);
            return;
        }

        this.filteredChannels.forEach(channelData => {
            const channelRow = this.createChannelRow(channelData);
            channelsContainer.appendChild(channelRow);
        });
    }

    createChannelRow(channelData) {
        const row = document.createElement('div');
        row.className = 'epg-channel-row';

        // Channel info
        const channelInfo = document.createElement('div');
        channelInfo.className = 'epg-channel-info';
        channelInfo.innerHTML = `
            <div class="epg-channel-name">${this.escapeHtml(channelData.channel.channel_name)}</div>
            <div class="epg-channel-id">${this.escapeHtml(channelData.channel.channel_id)}</div>
        `;

        // Programs
        const programsContainer = document.createElement('div');
        programsContainer.className = 'epg-programs';

        this.renderPrograms(programsContainer, channelData.programs);

        row.appendChild(channelInfo);
        row.appendChild(programsContainer);

        return row;
    }

    renderPrograms(container, programs) {
        const startTime = new Date(this.currentStartTime);
        const endTime = new Date(startTime);
        endTime.setHours(endTime.getHours() + this.timeRangeHours);

        // Sort programs by start time
        const sortedPrograms = programs.sort((a, b) =>
            new Date(a.start_time) - new Date(b.start_time)
        );

        const timeSlotDuration = 30; // 30 minutes per slot
        const currentTime = new Date(startTime);
        let programIndex = 0;

        while (currentTime < endTime) {
            const slotEndTime = new Date(currentTime);
            slotEndTime.setMinutes(slotEndTime.getMinutes() + timeSlotDuration);

            // Find program that overlaps with this time slot
            const program = this.findProgramForTimeSlot(sortedPrograms, currentTime, slotEndTime);

            if (program) {
                const programElement = this.createProgramElement(program, currentTime, slotEndTime);
                container.appendChild(programElement);
            } else {
                const emptySlot = this.createEmptySlot();
                container.appendChild(emptySlot);
            }

            currentTime.setMinutes(currentTime.getMinutes() + timeSlotDuration);
        }
    }

    findProgramForTimeSlot(programs, slotStart, slotEnd) {
        return programs.find(program => {
            const programStart = new Date(program.start_time);
            const programEnd = new Date(program.end_time);

            // Program overlaps with time slot
            return programStart < slotEnd && programEnd > slotStart;
        });
    }

    createProgramElement(program, slotStart, slotEnd) {
        const programDiv = document.createElement('div');
        programDiv.className = 'epg-program';

        // Check if program is currently airing
        const now = new Date();
        const programStart = new Date(program.start_time);
        const programEnd = new Date(program.end_time);

        if (now >= programStart && now <= programEnd) {
            programDiv.className += ' current';
        }

        const timeText = `${this.formatTime(programStart)} - ${this.formatTime(programEnd)}`;

        programDiv.innerHTML = `
            <div class="epg-program-time">${timeText}</div>
            <div class="epg-program-title">${this.escapeHtml(program.program_title)}</div>
            ${program.program_category ?
                `<div class="epg-program-category">${this.escapeHtml(program.program_category)}</div>` : ''
            }
        `;

        // Add tooltip events
        programDiv.addEventListener('mouseenter', (e) => {
            this.showTooltip(e, program);
        });

        programDiv.addEventListener('mouseleave', () => {
            this.hideTooltip();
        });

        return programDiv;
    }

    createEmptySlot() {
        const emptyDiv = document.createElement('div');
        emptyDiv.className = 'epg-empty-slot';
        emptyDiv.textContent = 'No Program';
        return emptyDiv;
    }

    showTooltip(event, program) {
        if (!this.tooltip) return;

        const programStart = new Date(program.start_time);
        const programEnd = new Date(program.end_time);
        const duration = Math.round((programEnd - programStart) / (1000 * 60)); // minutes

        let content = `
            <div><strong>${this.escapeHtml(program.program_title)}</strong></div>
            <div style="margin: 0.5rem 0;">
                ${this.formatTime(programStart)} - ${this.formatTime(programEnd)} (${duration} min)
            </div>
        `;

        if (program.program_description) {
            content += `<div style="margin: 0.5rem 0;">${this.escapeHtml(program.program_description)}</div>`;
        }

        if (program.program_category) {
            content += `<div style="margin: 0.5rem 0;"><strong>Category:</strong> ${this.escapeHtml(program.program_category)}</div>`;
        }

        if (program.episode_num || program.season_num) {
            let episodeInfo = '';
            if (program.season_num) episodeInfo += `Season ${program.season_num}`;
            if (program.episode_num) {
                if (episodeInfo) episodeInfo += ', ';
                episodeInfo += `Episode ${program.episode_num}`;
            }
            content += `<div style="margin: 0.5rem 0;"><strong>Episode:</strong> ${episodeInfo}</div>`;
        }

        if (program.rating) {
            content += `<div style="margin: 0.5rem 0;"><strong>Rating:</strong> ${this.escapeHtml(program.rating)}</div>`;
        }

        this.tooltip.innerHTML = content;

        // Position tooltip
        const rect = event.target.getBoundingClientRect();
        this.tooltip.style.left = `${rect.left + window.scrollX}px`;
        this.tooltip.style.top = `${rect.top + window.scrollY - this.tooltip.offsetHeight - 10}px`;

        // Show tooltip
        this.tooltip.classList.add('show');
    }

    hideTooltip() {
        if (this.tooltip) {
            this.tooltip.classList.remove('show');
        }
    }

    jumpToNow() {
        const now = new Date();

        // Set date to today
        const dateStr = now.toISOString().split('T')[0];
        document.getElementById('dateSelect').value = dateStr;
        this.currentDate = new Date(dateStr + 'T00:00:00');

        // Set start time to current hour
        const currentHour = now.getHours();
        const timeStr = String(currentHour).padStart(2, '0') + ':00';
        document.getElementById('startTime').value = timeStr;

        this.currentStartTime = new Date(this.currentDate);
        this.currentStartTime.setHours(currentHour, 0, 0, 0);

        // Reload data
        this.loadEpgData();
    }

    formatTime(date) {
        return date.toLocaleTimeString([], {
            hour: '2-digit',
            minute: '2-digit',
            hour12: false
        });
    }

    showLoading() {
        document.getElementById('epgLoading').style.display = 'flex';
        document.getElementById('epgNoData').style.display = 'none';
        document.getElementById('epgContent').style.display = 'none';
    }

    hideLoading() {
        document.getElementById('epgLoading').style.display = 'none';
    }

    showNoData() {
        document.getElementById('epgLoading').style.display = 'none';
        document.getElementById('epgNoData').style.display = 'flex';
        document.getElementById('epgContent').style.display = 'none';
    }

    showContent() {
        document.getElementById('epgLoading').style.display = 'none';
        document.getElementById('epgNoData').style.display = 'none';
        document.getElementById('epgContent').style.display = 'block';
    }

    showError(message) {
        // Create alert element
        const alert = document.createElement('div');
        alert.className = 'alert alert-danger alert-dismissible fade show';
        alert.innerHTML = `
            ${message}
            <button type="button" class="close" data-dismiss="alert">
                <span>&times;</span>
            </button>
        `;

        // Add to alerts container
        const container = document.getElementById('alertsContainer') || document.body;
        container.appendChild(alert);

        // Auto-hide after 10 seconds
        setTimeout(() => {
            if (alert.parentNode) {
                alert.remove();
            }
        }, 10000);

        // Handle close button
        alert.querySelector('.close').addEventListener('click', () => {
            alert.remove();
        });
    }

    escapeHtml(text) {
        if (!text) return '';
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

// Initialize when page loads
let epgViewer;
document.addEventListener('DOMContentLoaded', () => {
    epgViewer = new EpgViewer();
    window.epgViewer = epgViewer;
});
