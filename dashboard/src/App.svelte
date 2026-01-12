<script>
  import { onMount } from 'svelte';

  let instances = $state([]);
  let logs = $state([]);
  let metrics = $state('');
  let activeTab = $state('instances');
  let logStream = $state(null);
  let loading = $state(true);
  let error = $state(null);

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

  // Fetch metrics
  async function fetchMetrics() {
    try {
      const res = await fetch('/metrics');
      metrics = await res.text();
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
      logs = [entry, ...logs.slice(0, 99)];
    };
    logStream.onerror = () => {
      logStream.close();
      logStream = null;
    };
  }

  // Stop log streaming
  function stopLogStream() {
    if (logStream) {
      logStream.close();
      logStream = null;
    }
  }

  // Refresh data based on active tab
  function refreshData() {
    loading = true;
    error = null;

    if (activeTab === 'instances') {
      fetchInstances().finally(() => loading = false);
    } else if (activeTab === 'logs') {
      fetchLogs().finally(() => loading = false);
    } else if (activeTab === 'metrics') {
      fetchMetrics().finally(() => loading = false);
    }
  }

  // Format uptime
  function formatUptime(secs) {
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
    return `${Math.floor(secs / 86400)}d`;
  }

  // Format timestamp
  function formatTime(ts) {
    return new Date(ts).toLocaleTimeString();
  }

  // Health status color
  function healthColor(status) {
    switch (status) {
      case 'healthy': return 'text-green-600';
      case 'degraded': return 'text-yellow-600';
      case 'unhealthy': return 'text-red-600';
      default: return 'text-gray-600';
    }
  }

  // Format bytes to human-readable (e.g., "134MB")
  function formatBytes(bytes) {
    const KB = 1024;
    const MB = KB * 1024;
    const GB = MB * 1024;
    if (bytes >= GB) return `${(bytes / GB).toFixed(1)}GB`;
    if (bytes >= MB) return `${Math.floor(bytes / MB)}MB`;
    if (bytes >= KB) return `${Math.floor(bytes / KB)}KB`;
    return `${bytes}B`;
  }

  // Format storage display (e.g., "134MB / 512MB" or "134MB" if no quota)
  function formatStorage(inst) {
    const used = formatBytes(inst.storage_used_bytes || 0);
    if (inst.storage_quota_bytes) {
      return `${used} / ${formatBytes(inst.storage_quota_bytes)}`;
    }
    return used;
  }

  // Storage usage color based on percentage
  function storageColor(inst) {
    if (!inst.storage_quota_bytes) return 'text-gray-500';
    const ratio = (inst.storage_used_bytes || 0) / inst.storage_quota_bytes;
    if (ratio > 0.9) return 'text-red-600';
    if (ratio > 0.7) return 'text-yellow-600';
    return 'text-green-600';
  }

  // Handle tab change
  function selectTab(tab) {
    activeTab = tab;
    if (tab === 'logs') {
      startLogStream();
    } else {
      stopLogStream();
    }
    refreshData();
  }

  onMount(() => {
    refreshData();

    // Poll for instances every 5s
    const interval = setInterval(() => {
      if (activeTab === 'instances') fetchInstances();
    }, 5000);

    return () => {
      clearInterval(interval);
      stopLogStream();
    };
  });
</script>

<div class="min-h-screen bg-gray-50">
  <!-- Header -->
  <header class="bg-white shadow-sm">
    <div class="max-w-7xl mx-auto px-4 py-4 flex items-center justify-between">
      <h1 class="text-xl font-semibold text-gray-900">tenement</h1>
      <span class="text-sm text-gray-500">process hypervisor</span>
    </div>
  </header>

  <!-- Tabs -->
  <div class="max-w-7xl mx-auto px-4 mt-4">
    <div class="border-b border-gray-200">
      <nav class="-mb-px flex space-x-8">
        <button
          onclick={() => selectTab('instances')}
          class="py-2 px-1 border-b-2 font-medium text-sm {activeTab === 'instances' ? 'border-blue-500 text-blue-600' : 'border-transparent text-gray-500 hover:text-gray-700'}"
        >
          Instances
        </button>
        <button
          onclick={() => selectTab('logs')}
          class="py-2 px-1 border-b-2 font-medium text-sm {activeTab === 'logs' ? 'border-blue-500 text-blue-600' : 'border-transparent text-gray-500 hover:text-gray-700'}"
        >
          Logs
        </button>
        <button
          onclick={() => selectTab('metrics')}
          class="py-2 px-1 border-b-2 font-medium text-sm {activeTab === 'metrics' ? 'border-blue-500 text-blue-600' : 'border-transparent text-gray-500 hover:text-gray-700'}"
        >
          Metrics
        </button>
      </nav>
    </div>
  </div>

  <!-- Content -->
  <main class="max-w-7xl mx-auto px-4 py-6">
    {#if error}
      <div class="bg-red-50 text-red-700 p-4 rounded-md mb-4">
        Error: {error}
      </div>
    {/if}

    {#if loading}
      <div class="text-gray-500">Loading...</div>
    {:else if activeTab === 'instances'}
      <!-- Instances Table -->
      {#if instances.length === 0}
        <div class="text-gray-500 text-center py-8">No instances running</div>
      {:else}
        <div class="bg-white shadow rounded-lg overflow-hidden">
          <table class="min-w-full divide-y divide-gray-200">
            <thead class="bg-gray-50">
              <tr>
                <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Instance</th>
                <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Socket</th>
                <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Uptime</th>
                <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Restarts</th>
                <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Storage</th>
                <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Health</th>
              </tr>
            </thead>
            <tbody class="bg-white divide-y divide-gray-200">
              {#each instances as inst}
                <tr>
                  <td class="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">{inst.id}</td>
                  <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500 font-mono">{inst.socket}</td>
                  <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">{formatUptime(inst.uptime_secs)}</td>
                  <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">{inst.restarts}</td>
                  <td class="px-6 py-4 whitespace-nowrap text-sm {storageColor(inst)}">{formatStorage(inst)}</td>
                  <td class="px-6 py-4 whitespace-nowrap text-sm {healthColor(inst.health)}">{inst.health}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}

    {:else if activeTab === 'logs'}
      <!-- Logs -->
      <div class="flex justify-between items-center mb-4">
        <span class="text-sm text-gray-500">
          {logStream ? 'Streaming live...' : 'Log stream disconnected'}
        </span>
        <button
          onclick={() => logStream ? stopLogStream() : startLogStream()}
          class="text-sm text-blue-600 hover:text-blue-800"
        >
          {logStream ? 'Stop' : 'Start'} streaming
        </button>
      </div>
      <div class="bg-gray-900 rounded-lg p-4 font-mono text-sm overflow-auto max-h-[600px]">
        {#if logs.length === 0}
          <div class="text-gray-500">No logs available</div>
        {:else}
          {#each logs as log}
            <div class="py-0.5">
              <span class="text-gray-500">{formatTime(log.timestamp)}</span>
              <span class="text-blue-400">[{log.process}:{log.instance_id}]</span>
              <span class="{log.level === 'stderr' ? 'text-red-400' : 'text-green-400'}">{log.level}</span>
              <span class="text-gray-300">{log.message}</span>
            </div>
          {/each}
        {/if}
      </div>

    {:else if activeTab === 'metrics'}
      <!-- Metrics -->
      <div class="bg-white shadow rounded-lg p-6">
        <pre class="text-sm text-gray-700 overflow-auto">{metrics || 'No metrics available'}</pre>
      </div>
    {/if}
  </main>
</div>
