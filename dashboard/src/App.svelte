<script>
  import { onMount } from 'svelte';

  let instances = $state([]);
  let logs = $state([]);
  let telemetry = $state(null);
  let activeTab = $state('overview');
  let logStream = $state(null);
  let loading = $state(true);
  let error = $state(null);

  // Fetch telemetry (structured JSON)
  async function fetchTelemetry() {
    try {
      const res = await fetch('/api/telemetry');
      telemetry = await res.json();
      // Also populate instances from telemetry
      if (telemetry && telemetry.instances) {
        instances = telemetry.instances;
      }
    } catch (e) {
      error = e.message;
    }
  }

  // Fetch instances
  async function fetchInstances() {
    try {
      const res = await fetch('/api/instances');
      instances = await res.json();
    } catch (e) {
      error = e.message;
    }
  }

  // Fetch logs
  async function fetchLogs(limit = 100) {
    try {
      const res = await fetch(`/api/logs?limit=${limit}`);
      logs = await res.json();
    } catch (e) {
      error = e.message;
    }
  }

  // Start log streaming
  function startLogStream() {
    if (logStream) logStream.close();
    logStream = new EventSource('/api/logs/stream');
    logStream.onmessage = (event) => {
      const entry = JSON.parse(event.data);
      logs = [entry, ...logs.slice(0, 199)];
    };
    logStream.onerror = () => {
      logStream.close();
      logStream = null;
    };
  }

  function stopLogStream() {
    if (logStream) {
      logStream.close();
      logStream = null;
    }
  }

  function refreshData() {
    loading = true;
    error = null;
    const p = activeTab === 'logs'
      ? fetchLogs()
      : fetchTelemetry();
    p.finally(() => loading = false);
  }

  function formatUptime(secs) {
    if (!secs && secs !== 0) return '-';
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
    return `${Math.floor(secs / 86400)}d`;
  }

  function formatTime(ts) {
    return new Date(ts).toLocaleTimeString();
  }

  function formatBytes(bytes) {
    if (!bytes) return '0B';
    const KB = 1024, MB = KB * 1024, GB = MB * 1024;
    if (bytes >= GB) return `${(bytes / GB).toFixed(1)}GB`;
    if (bytes >= MB) return `${Math.floor(bytes / MB)}MB`;
    if (bytes >= KB) return `${Math.floor(bytes / KB)}KB`;
    return `${bytes}B`;
  }

  function healthColor(status) {
    if (status === 'healthy') return '#22c55e';
    if (status === 'degraded') return '#eab308';
    if (status === 'unhealthy' || status === 'failed') return '#ef4444';
    return '#6b7280';
  }

  function healthBg(status) {
    if (status === 'healthy') return 'bg-green-100 text-green-800';
    if (status === 'degraded') return 'bg-yellow-100 text-yellow-800';
    if (status === 'unhealthy' || status === 'failed') return 'bg-red-100 text-red-800';
    return 'bg-gray-100 text-gray-800';
  }

  function selectTab(tab) {
    activeTab = tab;
    if (tab === 'logs') {
      startLogStream();
      fetchLogs();
    } else {
      stopLogStream();
    }
    refreshData();
  }

  onMount(() => {
    refreshData();
    const interval = setInterval(fetchTelemetry, 5000);
    return () => {
      clearInterval(interval);
      stopLogStream();
    };
  });
</script>

<div class="min-h-screen bg-gray-950 text-gray-100">
  <!-- Header -->
  <header class="border-b border-gray-800 bg-gray-900">
    <div class="max-w-7xl mx-auto px-6 py-4 flex items-center justify-between">
      <div class="flex items-center gap-3">
        <h1 class="text-lg font-semibold text-white tracking-tight">tenement</h1>
        <span class="text-xs text-gray-500 bg-gray-800 px-2 py-0.5 rounded">dashboard</span>
      </div>
      {#if telemetry}
        <div class="flex gap-6 text-sm">
          <div>
            <span class="text-gray-500">instances</span>
            <span class="ml-1 font-mono text-white">{telemetry.summary.total_instances}</span>
          </div>
          <div>
            <span class="text-gray-500">healthy</span>
            <span class="ml-1 font-mono text-green-400">{telemetry.summary.healthy_instances}</span>
          </div>
          <div>
            <span class="text-gray-500">requests</span>
            <span class="ml-1 font-mono text-blue-400">{telemetry.summary.total_requests.toLocaleString()}</span>
          </div>
        </div>
      {/if}
    </div>
  </header>

  <!-- Tabs -->
  <div class="max-w-7xl mx-auto px-6 mt-4">
    <nav class="flex gap-1 bg-gray-900 rounded-lg p-1 w-fit">
      {#each ['overview', 'instances', 'logs'] as tab}
        <button
          onclick={() => selectTab(tab)}
          class="px-4 py-1.5 text-sm rounded-md transition-colors {activeTab === tab ? 'bg-gray-700 text-white' : 'text-gray-400 hover:text-gray-200'}"
        >
          {tab.charAt(0).toUpperCase() + tab.slice(1)}
        </button>
      {/each}
    </nav>
  </div>

  <!-- Content -->
  <main class="max-w-7xl mx-auto px-6 py-6">
    {#if error}
      <div class="bg-red-900/30 text-red-400 p-4 rounded-lg mb-4 text-sm border border-red-800">
        {error}
      </div>
    {/if}

    {#if activeTab === 'overview'}
      <!-- Overview: health grid + per-instance telemetry -->
      {#if !telemetry || instances.length === 0}
        <div class="text-gray-500 text-center py-16">No instances running</div>
      {:else}
        <!-- Health grid -->
        <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-3 mb-8">
          {#each instances as inst}
            <div class="bg-gray-900 border border-gray-800 rounded-lg p-3">
              <div class="flex items-center justify-between mb-2">
                <span class="text-xs font-mono text-gray-300 truncate">{inst.instance || inst.id}</span>
                <span class="w-2 h-2 rounded-full" style="background-color: {healthColor(inst.health)}"></span>
              </div>
              <div class="text-xs text-gray-500 space-y-0.5">
                <div class="flex justify-between">
                  <span>reqs</span>
                  <span class="font-mono text-gray-300">{(inst.requests_total || 0).toLocaleString()}</span>
                </div>
                <div class="flex justify-between">
                  <span>avg</span>
                  <span class="font-mono text-gray-300">{(inst.request_duration_avg_ms || 0).toFixed(1)}ms</span>
                </div>
                <div class="flex justify-between">
                  <span>up</span>
                  <span class="font-mono text-gray-300">{formatUptime(inst.uptime_secs)}</span>
                </div>
              </div>
            </div>
          {/each}
        </div>

        <!-- Detailed table -->
        <div class="bg-gray-900 border border-gray-800 rounded-lg overflow-hidden">
          <table class="min-w-full">
            <thead>
              <tr class="border-b border-gray-800">
                <th class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Instance</th>
                <th class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Health</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Requests</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Avg Latency</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Uptime</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Idle</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Restarts</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Weight</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Storage</th>
              </tr>
            </thead>
            <tbody>
              {#each instances as inst}
                <tr class="border-b border-gray-800/50 hover:bg-gray-800/30">
                  <td class="px-4 py-3 text-sm font-mono text-gray-200">{inst.id || `${inst.process}:${inst.instance}`}</td>
                  <td class="px-4 py-3">
                    <span class="text-xs px-2 py-0.5 rounded-full {healthBg(inst.health)}">{inst.health}</span>
                  </td>
                  <td class="px-4 py-3 text-sm text-right font-mono text-gray-300">{(inst.requests_total || 0).toLocaleString()}</td>
                  <td class="px-4 py-3 text-sm text-right font-mono text-gray-300">{(inst.request_duration_avg_ms || 0).toFixed(1)}ms</td>
                  <td class="px-4 py-3 text-sm text-right text-gray-400">{formatUptime(inst.uptime_secs)}</td>
                  <td class="px-4 py-3 text-sm text-right text-gray-400">{formatUptime(inst.idle_secs)}</td>
                  <td class="px-4 py-3 text-sm text-right text-gray-400">{inst.restarts}</td>
                  <td class="px-4 py-3 text-sm text-right text-gray-400">{inst.weight}</td>
                  <td class="px-4 py-3 text-sm text-right text-gray-400">{formatBytes(inst.storage_used_bytes)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}

    {:else if activeTab === 'instances'}
      <!-- Instances: same as overview table but focused -->
      {#if instances.length === 0}
        <div class="text-gray-500 text-center py-16">No instances running</div>
      {:else}
        <div class="bg-gray-900 border border-gray-800 rounded-lg overflow-hidden">
          <table class="min-w-full">
            <thead>
              <tr class="border-b border-gray-800">
                <th class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Instance</th>
                <th class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Health</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Uptime</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Idle</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Restarts</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Weight</th>
                <th class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase">Storage</th>
              </tr>
            </thead>
            <tbody>
              {#each instances as inst}
                <tr class="border-b border-gray-800/50 hover:bg-gray-800/30">
                  <td class="px-4 py-3 text-sm font-mono text-gray-200">{inst.id || `${inst.process}:${inst.instance}`}</td>
                  <td class="px-4 py-3">
                    <span class="text-xs px-2 py-0.5 rounded-full {healthBg(inst.health)}">{inst.health}</span>
                  </td>
                  <td class="px-4 py-3 text-sm text-right text-gray-400">{formatUptime(inst.uptime_secs)}</td>
                  <td class="px-4 py-3 text-sm text-right text-gray-400">{formatUptime(inst.idle_secs)}</td>
                  <td class="px-4 py-3 text-sm text-right text-gray-400">{inst.restarts}</td>
                  <td class="px-4 py-3 text-sm text-right text-gray-400">{inst.weight}</td>
                  <td class="px-4 py-3 text-sm text-right text-gray-400">{formatBytes(inst.storage_used_bytes)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}

    {:else if activeTab === 'logs'}
      <!-- Logs -->
      <div class="flex justify-between items-center mb-3">
        <span class="text-sm text-gray-500">
          {logStream ? 'Streaming live' : 'Disconnected'}
          <span class="inline-block w-2 h-2 rounded-full ml-1 {logStream ? 'bg-green-500' : 'bg-gray-600'}"></span>
        </span>
        <button
          onclick={() => logStream ? stopLogStream() : startLogStream()}
          class="text-sm px-3 py-1 rounded bg-gray-800 text-gray-300 hover:bg-gray-700 transition-colors"
        >
          {logStream ? 'Stop' : 'Start'}
        </button>
      </div>
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4 font-mono text-xs overflow-auto max-h-[700px] leading-relaxed">
        {#if logs.length === 0}
          <div class="text-gray-600">No logs available</div>
        {:else}
          {#each logs as log}
            <div class="py-0.5 hover:bg-gray-800/30 px-1 -mx-1 rounded">
              <span class="text-gray-600">{formatTime(log.timestamp)}</span>
              <span class="text-blue-400 ml-1">{log.process}:{log.instance_id}</span>
              <span class="ml-1 {log.level === 'stderr' ? 'text-red-400' : 'text-gray-500'}">{log.level === 'stderr' ? 'ERR' : 'OUT'}</span>
              <span class="text-gray-300 ml-1">{log.message}</span>
            </div>
          {/each}
        {/if}
      </div>
    {/if}
  </main>
</div>
