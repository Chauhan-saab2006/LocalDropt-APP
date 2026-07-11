// ── Tauri globals ──────────────────────────────────────────────────────────
const { invoke } = window.__TAURI__.core;
const { listen  } = window.__TAURI__.event;

// ── DOM refs ───────────────────────────────────────────────────────────────
const statusDotEl       = document.getElementById('status-dot');
const statusTextEl      = document.getElementById('status-text');
const qrPlaceholderEl   = document.getElementById('qr-placeholder');
const qrReadyEl         = document.getElementById('qr-ready');
const qrImageEl         = document.getElementById('qr-image');
const serverUrlTextEl   = document.getElementById('server-url-text');
const copyUrlBtn        = document.getElementById('copy-url-btn');
const copyBtnLabel      = document.getElementById('copy-btn-label');
const filesEmptyEl      = document.getElementById('files-empty');
const filesListEl       = document.getElementById('files-list');
const filesCountEl      = document.getElementById('files-count');
const btnAddFile        = document.getElementById('btn-add-file');
const transfersSectionEl = document.getElementById('transfers-section');
const transfersListEl   = document.getElementById('transfers-list');
const btnClearTransfers = document.getElementById('btn-clear-transfers');

// ── App state ──────────────────────────────────────────────────────────────
let sharedFiles = [];
const transfers = new Map();   // key: file_name → { el }

// ── Utilities ──────────────────────────────────────────────────────────────
function formatBytes(bytes) {
  if (!bytes || bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

function fileIcon(name) {
  const ext = (name || '').split('.').pop().toLowerCase();
  const map = {
    jpg:'🖼️', jpeg:'🖼️', png:'🖼️', gif:'🖼️', webp:'🖼️', svg:'🖼️', bmp:'🖼️', heic:'🖼️',
    mp4:'🎬', mov:'🎬', avi:'🎬', mkv:'🎬', webm:'🎬',
    mp3:'🎵', wav:'🎵', flac:'🎵', aac:'🎵', ogg:'🎵',
    pdf:'📕', doc:'📝', docx:'📝', xls:'📊', xlsx:'📊', ppt:'📽️', pptx:'📽️',
    zip:'📦', rar:'📦', '7z':'📦', tar:'📦', gz:'📦',
    exe:'⚙️', msi:'⚙️', apk:'📱', dmg:'💿', iso:'💿',
    txt:'📃', md:'📃', json:'📋', xml:'📋', csv:'📋',
    js:'🟨', ts:'🟦', py:'🐍', rs:'🦀', html:'🌐', css:'🎨',
  };
  return map[ext] || '📄';
}

// ── Server Info ────────────────────────────────────────────────────────────
function applyServerInfo(url, qrPngB64) {
  qrPlaceholderEl.style.display = 'none';
  qrImageEl.src = 'data:image/png;base64,' + qrPngB64;
  qrReadyEl.style.display = 'block';
  serverUrlTextEl.textContent = url;

  statusDotEl.className = 'status-dot green';
  statusTextEl.textContent = 'Server running  ·  ' + url;
}

// ── Shared Files ───────────────────────────────────────────────────────────
function renderSharedFiles() {
  filesCountEl.textContent = sharedFiles.length;

  if (sharedFiles.length === 0) {
    filesEmptyEl.style.display = 'flex';
    filesListEl.style.display = 'none';
    return;
  }

  filesEmptyEl.style.display = 'none';
  filesListEl.style.display = 'flex';
  filesListEl.innerHTML = '';

  sharedFiles.forEach(file => {
    const card = document.createElement('div');
    card.className = 'shared-file-card';
    card.innerHTML = `
      <div class="sfc-icon">${fileIcon(file.name)}</div>
      <div class="sfc-info">
        <div class="sfc-name">${file.name}</div>
        <div class="sfc-size">${formatBytes(file.size)}</div>
      </div>
      <button class="sfc-remove" data-id="${file.id}" title="Stop sharing this file">
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
          <line x1="18" y1="6" x2="6" y2="18"/>
          <line x1="6" y1="6" x2="18" y2="18"/>
        </svg>
      </button>
    `;
    card.querySelector('.sfc-remove').addEventListener('click', async () => {
      try {
        await invoke('remove_shared_file', { id: file.id });
        sharedFiles = sharedFiles.filter(f => f.id !== file.id);
        renderSharedFiles();
      } catch (err) {
        console.error('Remove file error:', err);
      }
    });
    filesListEl.appendChild(card);
  });
}

async function addFile() {
  try {
    const paths = await invoke('select_file');   // now returns String[]
    if (!paths || paths.length === 0) return;

    // Add all picked files, collect results
    const results = await Promise.all(
      paths.map(path => invoke('add_shared_file', { path }).catch(err => {
        console.error('Failed to add', path, err);
        return null;
      }))
    );

    results.forEach(sf => { if (sf) sharedFiles.push(sf); });
    renderSharedFiles();
  } catch (err) {
    console.error('Add file error:', err);
  }
}

// ── Upload Transfers ───────────────────────────────────────────────────────
function updateTransfer(progress) {
  const key = progress.file_name;

  if (!transfers.has(key)) {
    transfersSectionEl.style.display = 'block';
    const item = document.createElement('div');
    item.className = 'transfer-item';
    transfersListEl.insertBefore(item, transfersListEl.firstChild);
    transfers.set(key, { el: item });
  }

  const { el } = transfers.get(key);

  const pct = progress.total_bytes > 0
    ? Math.min(100, Math.round((progress.bytes_received / progress.total_bytes) * 100))
    : (progress.status === 'completed' ? 100 : 0);

  let icon = '⬆️';
  let cls  = 'transfer-item';
  if (progress.status === 'completed') { icon = '✅'; cls += ' completed'; }
  else if (progress.status === 'error')  { icon = '❌'; cls += ' error'; }
  else                                   { cls += ' active'; }

  el.className = cls;
  el.innerHTML = `
    <div class="ti-header">
      <span class="ti-icon">${icon}</span>
      <span class="ti-name">${progress.file_name}</span>
      <span class="ti-speed">${progress.speed_mbps.toFixed(1)} MB/s</span>
    </div>
    <div class="ti-bar"><div class="ti-fill" style="width:${pct}%"></div></div>
    <div class="ti-footer">
      <span class="ti-bytes">
        ${formatBytes(progress.bytes_received)}
        ${progress.total_bytes > 0 ? ' / ' + formatBytes(progress.total_bytes) : ''}
      </span>
      <span class="ti-pct">${pct}%</span>
    </div>
    <div class="ti-msg">${progress.message}</div>
  `;
}

// ── Init ───────────────────────────────────────────────────────────────────
async function init() {
  // 1. Try fetching server info (may already be up)
  try {
    const info = await invoke('get_server_info');
    if (info) applyServerInfo(info.url, info.qr_png_b64);
  } catch (e) {
    console.warn('get_server_info:', e);
  }

  // 2. Fetch initial shared-file list
  try {
    sharedFiles = await invoke('get_shared_files');
    renderSharedFiles();
  } catch (e) {
    console.warn('get_shared_files:', e);
  }

  // 3. Listen for server-started event (fires once on startup)
  await listen('server-started', event => {
    const { url, qr_png_b64 } = event.payload;
    applyServerInfo(url, qr_png_b64);
  });

  // 4. Listen for upload progress events
  await listen('upload-progress', event => {
    updateTransfer(event.payload);
  });

  // 5. Copy URL button
  copyUrlBtn.addEventListener('click', async () => {
    const url = serverUrlTextEl.textContent;
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
      copyBtnLabel.textContent = 'Copied!';
      copyUrlBtn.classList.add('copied');
      setTimeout(() => {
        copyBtnLabel.textContent = 'Copy';
        copyUrlBtn.classList.remove('copied');
      }, 2000);
    } catch (e) {
      console.warn('Clipboard write failed:', e);
    }
  });

  // 6. Add file button
  btnAddFile.addEventListener('click', addFile);

  // 7. Clear transfers
  btnClearTransfers.addEventListener('click', () => {
    transfersListEl.innerHTML = '';
    transfers.clear();
    transfersSectionEl.style.display = 'none';
  });
}

document.addEventListener('DOMContentLoaded', init);
