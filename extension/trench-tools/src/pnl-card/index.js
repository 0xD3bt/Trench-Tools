const CHANNEL_OUT = "trench-tools-pnl-card";
const CHANNEL_IN = "trench-tools-content";
const PARENT_ORIGIN = (() => {
  try {
    return new URLSearchParams(window.location.search).get("parentOrigin") || "";
  } catch {
    return "";
  }
})();
let parentPort = null;

const MAX_VIDEO_DURATION_SECONDS = 60;
const VIDEO_THUMBNAIL_SEEK_SECONDS = 0.4;
const VIDEO_EXPORT_FPS = 30;
const MAX_IMAGE_BYTES = 6 * 1024 * 1024;
const MAX_VIDEO_BYTES = 50 * 1024 * 1024;
const SUPPORTED_IMAGE_TYPES = new Set(["image/png", "image/jpeg", "image/webp"]);
const SUPPORTED_VIDEO_TYPES = new Set(["video/mp4", "video/webm"]);
const VIDEO_EXPORT_MIME_TYPES = [
  "video/mp4;codecs=avc1.42E01E,mp4a.40.2",
  "video/mp4;codecs=avc1,mp4a.40.2",
  "video/mp4;codecs=avc1.42E01E",
  "video/mp4;codecs=avc1",
  "video/mp4",
  "video/webm;codecs=vp9,opus",
  "video/webm;codecs=vp8,opus",
  "video/webm;codecs=vp9",
  "video/webm;codecs=vp8",
  "video/webm"
];

const BUILT_IN_TEMPLATES = [
  {
    id: "bear-template",
    kind: "image",
    name: "Bear",
    url: "../../assets/pnl-cards/bear-template.png",
    thumbnailUrl: "../../assets/pnl-cards/bear-template.png"
  },
  {
    id: "bear-cliff",
    kind: "image",
    name: "Bear Cliff",
    url: "../../assets/pnl-cards/bear-cliff.png",
    thumbnailUrl: "../../assets/pnl-cards/bear-cliff.png"
  },
  {
    id: "valley-claws",
    kind: "image",
    name: "Valley Claws",
    url: "../../assets/pnl-cards/valley-claws.png",
    thumbnailUrl: "../../assets/pnl-cards/valley-claws.png"
  },
  {
    id: "neon-tt",
    kind: "image",
    name: "Neon TT",
    url: "../../assets/pnl-cards/neon-tt.png",
    thumbnailUrl: "../../assets/pnl-cards/neon-tt.png"
  },
  {
    id: "perfect",
    kind: "video",
    name: "Perfect",
    url: "../../assets/pnl-cards/perfect.mp4",
    thumbnailUrl: "../../assets/pnl-cards/perfect-thumb.jpg"
  },
  {
    id: "switch",
    kind: "video",
    name: "Switch",
    url: "../../assets/pnl-cards/switch.mp4",
    thumbnailUrl: "../../assets/pnl-cards/switch-thumb.jpg"
  },
  {
    id: "lowkey",
    kind: "video",
    name: "Lowkey",
    url: "../../assets/pnl-cards/lowkey.mp4",
    thumbnailUrl: "../../assets/pnl-cards/lowkey-thumb.jpg"
  }
];

function solIconSvg(fill) {
  const color = String(fill || "#ffffff");
  return `data:image/svg+xml,${encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 11">` +
    `<path d="M2.6 0 L13.6 0 L11.4 2.8 L0.4 2.8 Z" fill="${color}"/>` +
    `<path d="M0.4 4.1 L11.4 4.1 L13.6 6.9 L2.6 6.9 Z" fill="${color}"/>` +
    `<path d="M2.6 8.2 L13.6 8.2 L11.4 11 L0.4 11 Z" fill="${color}"/>` +
    `</svg>`
  )}`;
}

function recolorImageToWhite(image) {
  const width = image.naturalWidth || image.width || 1;
  const height = image.naturalHeight || image.height || 1;
  const off = document.createElement("canvas");
  off.width = width;
  off.height = height;
  const ctx = off.getContext("2d");
  ctx.drawImage(image, 0, 0, width, height);
  ctx.globalCompositeOperation = "source-in";
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, width, height);
  return off;
}

function applyAlpha(color, alpha) {
  const hex = String(color || "").trim();
  const short = /^#([\da-f]{3})$/i.exec(hex);
  const long = /^#([\da-f]{6})$/i.exec(hex);
  let r;
  let g;
  let b;
  if (short) {
    r = parseInt(short[1][0] + short[1][0], 16);
    g = parseInt(short[1][1] + short[1][1], 16);
    b = parseInt(short[1][2] + short[1][2], 16);
  } else if (long) {
    r = parseInt(long[1].slice(0, 2), 16);
    g = parseInt(long[1].slice(2, 4), 16);
    b = parseInt(long[1].slice(4, 6), 16);
  } else {
    r = 255;
    g = 255;
    b = 255;
  }
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}
const CARD_WIDTH = 840;
const CARD_HEIGHT = 570;
const VIDEO_EXPORT_SIZES = [
  { id: "hd", label: "720P", width: 1280, height: 720, scene: "16:9" },
  { id: "full-hd", label: "1080P", width: 1920, height: 1080, scene: "16:9" }
];
const PROMO_LINE_ONE = "Own your execution.";
const PROMO_LINE_TWO = "Trade, deploy & snipe with 0% platform fees.";

const DEFAULT_SETTINGS = {
  templateId: "bear-template",
  mediaId: "",
  displayCurrency: "SOL",
  handle: "",
  mainTextColor: "#ffffff",
  positivePnlColor: "#2fe3ac",
  negativePnlColor: "#ec397a",
  rectangleTextColor: "#020307",
  bold: false,
  shadow: false,
  rectangle: true,
  volume: 0.7,
  muted: false,
  videoExportFps: VIDEO_EXPORT_FPS,
  videoExportSize: "hd",
  videoSectionOffset: 0
};

const state = {
  tokenContext: null,
  walletStatus: null,
  cardState: null,
  solUsd: null,
  downloadToken: "",
  settings: { ...DEFAULT_SETTINGS },
  mediaMode: "image",
  mediaDataUrls: new Map(),
  pendingMediaData: new Set(),
  mediaDataWaiters: new Map(),
  previewImageUrl: "",
  previewVideoUrl: "",
  previewBackgroundVideoUrl: "",
  previewVideoAspect: 16 / 9,
  pendingVideoMetadataUrl: "",
  pendingPreviewImage: "",
  pnlMode: "net",
  localSettingsDirty: false,
  localAudioSettingsDirty: false,
  exporting: false,
  exportProgress: 0
};

const elements = {
  preview: document.getElementById("card-preview"),
  previewCard: document.getElementById("card-preview-card"),
  previewBackgroundImage: document.getElementById("preview-background-image"),
  previewBackgroundVideo: document.getElementById("preview-background-video"),
  previewBackgroundScrim: document.querySelector(".preview-background-scrim"),
  mediaImage: document.getElementById("card-media-image"),
  mediaVideo: document.getElementById("card-media-video"),
  brandLogo: document.getElementById("brand-logo"),
  tokenName: document.getElementById("token-name"),
  pnlAmount: document.getElementById("pnl-amount"),
  pnlCurrencyIcon: document.getElementById("pnl-currency-icon"),
  pnlCurrencySymbol: document.getElementById("pnl-currency-symbol"),
  pnlAmountText: document.getElementById("pnl-amount-text"),
  pnlPercent: document.getElementById("pnl-percent"),
  investedAmount: document.getElementById("invested-amount"),
  positionAmount: document.getElementById("position-amount"),
  handle: document.getElementById("handle"),
  siteLabel: document.getElementById("site-label"),
  promoLineOne: document.getElementById("promo-line-one"),
  promoLineTwo: document.getElementById("promo-line-two"),
  modeImage: document.getElementById("mode-image"),
  modeVideo: document.getElementById("mode-video"),
  volumeControl: document.getElementById("volume-control"),
  volumeButton: document.getElementById("volume-button"),
  volumeIcon: document.getElementById("volume-icon"),
  volumeSlider: document.getElementById("volume-slider"),
  fpsButton: document.getElementById("fps-button"),
  videoSizeButton: document.getElementById("video-size-button"),
  mediaStrip: document.getElementById("media-strip"),
  mediaInput: document.getElementById("media-input"),
  currencyButton: document.getElementById("currency-button"),
  customizeButton: document.getElementById("customize-button"),
  customizePanel: document.getElementById("customize-panel"),
  customizeClose: document.getElementById("customize-close"),
  resyncButton: document.getElementById("resync-button"),
  downloadButton: document.getElementById("download-button"),
  copyButton: document.getElementById("copy-button"),
  notice: document.getElementById("notice"),
  handleInput: document.getElementById("handle-input"),
  mainColor: document.getElementById("main-color"),
  positiveColor: document.getElementById("positive-color"),
  negativeColor: document.getElementById("negative-color"),
  rectangleTextColor: document.getElementById("rectangle-text-color"),
  boldToggle: document.getElementById("bold-toggle"),
  shadowToggle: document.getElementById("shadow-toggle"),
  rectangleToggle: document.getElementById("rectangle-toggle"),
  saveCustomize: document.getElementById("save-customize")
};

if (typeof ResizeObserver !== "undefined") {
  const previewResizeObserver = new ResizeObserver(() => syncVideoSectionOffsetStyle());
  previewResizeObserver.observe(elements.preview);
}

window.addEventListener("message", (event) => {
  if (event.source !== window.parent) return;
  if (PARENT_ORIGIN && event.origin !== PARENT_ORIGIN) return;
  if (!event.data || event.data.channel !== CHANNEL_IN || event.data.type !== "pnl-card-connect") return;
  const port = event.ports?.[0];
  if (!port || parentPort) return;
  parentPort = port;
  parentPort.onmessage = handleParentMessage;
  parentPort.start?.();
  emit("pnl-card-ready");
});

elements.modeImage.addEventListener("click", () => setMediaMode("image"));
elements.modeVideo.addEventListener("click", () => setMediaMode("video"));
elements.fpsButton.addEventListener("click", () => {
  state.settings.videoExportFps = exportFps() === 60 ? 30 : 60;
  state.localSettingsDirty = true;
  render();
  persistSettings();
});
elements.videoSizeButton.addEventListener("click", () => {
  const sizes = VIDEO_EXPORT_SIZES.map((size) => size.id);
  const currentIndex = Math.max(0, sizes.indexOf(videoExportSize().id));
  state.settings.videoExportSize = sizes[(currentIndex + 1) % sizes.length];
  state.localSettingsDirty = true;
  render();
  persistSettings();
});
elements.previewCard.addEventListener("pointerdown", (event) => {
  if (state.mediaMode !== "video" || state.exporting) return;
  event.preventDefault();
  const startY = event.clientY;
  const startOffset = videoSectionOffset();
  const rangePx = videoSectionRangePx();
  const pointerId = event.pointerId;
  elements.previewCard.setPointerCapture?.(pointerId);
  const onMove = (moveEvent) => {
    const next = rangePx > 0
      ? startOffset + ((moveEvent.clientY - startY) / rangePx) * 100
      : startOffset;
    state.settings.videoSectionOffset = normalizeVideoSectionOffset(next);
    state.localSettingsDirty = true;
    render();
  };
  const onUp = () => {
    elements.previewCard.releasePointerCapture?.(pointerId);
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    persistSettings();
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp, { once: true });
});
elements.volumeButton.addEventListener("click", () => {
  if (state.settings.muted) {
    state.settings.muted = false;
    if (state.settings.volume <= 0) state.settings.volume = 0.5;
  } else {
    state.settings.muted = true;
  }
  state.localAudioSettingsDirty = true;
  applyVideoAudioState();
  syncVolumeUi();
  tryPlayPreviewVideo();
  persistSettings();
});
elements.volumeSlider.addEventListener("input", () => {
  const next = clampVolume(Number(elements.volumeSlider.value) / 100, DEFAULT_SETTINGS.volume);
  state.settings.volume = next;
  if (next > 0 && state.settings.muted) state.settings.muted = false;
  state.localAudioSettingsDirty = true;
  applyVideoAudioState();
  syncVolumeUi();
});
elements.volumeSlider.addEventListener("change", () => persistSettings());
elements.currencyButton.addEventListener("click", () => {
  state.settings.displayCurrency = state.settings.displayCurrency === "USD" ? "SOL" : "USD";
  persistSettings();
  render();
});
elements.customizeButton.addEventListener("click", () => {
  syncCustomizeInputs();
  elements.customizePanel.classList.remove("hidden");
});
elements.customizeClose.addEventListener("click", () => elements.customizePanel.classList.add("hidden"));

const COLOR_RESET_FIELDS = {
  "main-color": "mainTextColor",
  "positive-color": "positivePnlColor",
  "negative-color": "negativePnlColor",
  "rectangle-text-color": "rectangleTextColor"
};

for (const button of document.querySelectorAll(".color-reset")) {
  button.addEventListener("click", () => {
    const colorInputId = button.getAttribute("data-reset-color");
    const settingKey = COLOR_RESET_FIELDS[colorInputId];
    if (!colorInputId || !settingKey) return;
    const input = document.getElementById(colorInputId);
    if (!input) return;
    const defaultValue = DEFAULT_SETTINGS[settingKey];
    input.value = defaultValue;
    state.settings[settingKey] = defaultValue;
    updateColorHex(colorInputId, defaultValue);
    persistSettings();
    render();
  });
}
document.addEventListener("click", (event) => {
  if (event.button !== 0) return;
  const target = event.target;
  if (!(target instanceof Element)) return;
  const isBackdrop = target === document.documentElement
    || target === document.body
    || (target.tagName === "MAIN" && target.classList.contains("pnl-card-editor"));
  if (!isBackdrop) return;
  stopPreviewMedia();
  emit("pnl-card-close");
});
elements.resyncButton.addEventListener("click", () => {
  elements.resyncButton.disabled = true;
  setNotice("Resyncing PnL...");
  emit("pnl-card-resync-history");
});
elements.downloadButton.addEventListener("click", () => exportCard("export"));
elements.copyButton.addEventListener("click", () => {
  if (state.mediaMode !== "video") {
    exportCard("copy");
  }
});
elements.saveCustomize.addEventListener("click", () => {
  collectCustomizeInputs();
  persistProfile();
  persistSettings();
  elements.customizePanel.classList.add("hidden");
  render();
});
elements.mediaInput.addEventListener("change", () => {
  const file = elements.mediaInput.files?.[0];
  elements.mediaInput.value = "";
  if (file) uploadMedia(file);
});

elements.mediaStrip.addEventListener(
  "wheel",
  (event) => {
    const deltaY = event.deltaY || 0;
    const deltaX = event.deltaX || 0;
    if (deltaY === 0) return;
    if (Math.abs(deltaY) <= Math.abs(deltaX)) return;
    const maxScroll = elements.mediaStrip.scrollWidth - elements.mediaStrip.clientWidth;
    if (maxScroll <= 0) return;
    const current = elements.mediaStrip.scrollLeft;
    if ((deltaY > 0 && current >= maxScroll) || (deltaY < 0 && current <= 0)) return;
    event.preventDefault();
    elements.mediaStrip.scrollLeft = current + deltaY;
  },
  { passive: false }
);

elements.handleInput.addEventListener("input", () => {
  const stripped = elements.handleInput.value.replace(/@+/g, "");
  if (stripped !== elements.handleInput.value) {
    const caret = Math.max(0, elements.handleInput.selectionStart - (elements.handleInput.value.length - stripped.length));
    elements.handleInput.value = stripped;
    try {
      elements.handleInput.setSelectionRange(caret, caret);
    } catch (_error) {}
  }
  collectCustomizeInputs();
  render();
});

for (const input of [
  elements.mainColor,
  elements.positiveColor,
  elements.negativeColor,
  elements.rectangleTextColor,
  elements.boldToggle,
  elements.shadowToggle,
  elements.rectangleToggle
]) {
  input.addEventListener("input", () => {
    collectCustomizeInputs();
    if (input.type === "color") updateColorHex(input.id, input.value);
    render();
  });
  input.addEventListener("change", () => {
    collectCustomizeInputs();
    persistSettings();
  });
}

elements.handleInput.addEventListener("change", () => {
  collectCustomizeInputs();
  persistProfile();
  persistSettings();
});

function updateColorHex(inputId, value) {
  const node = document.querySelector(`.color-hex[data-hex-for="${inputId}"]`);
  if (!node) return;
  const hex = String(value || "").replace(/^#/, "").toUpperCase();
  node.textContent = hex.padEnd(6, "0").slice(0, 6);
}

function syncAllColorHexes() {
  updateColorHex("main-color", elements.mainColor.value);
  updateColorHex("positive-color", elements.positiveColor.value);
  updateColorHex("negative-color", elements.negativeColor.value);
  updateColorHex("rectangle-text-color", elements.rectangleTextColor.value);
}

function handleParentMessage(event) {
  if (!event.data || event.data.channel !== CHANNEL_IN) return;
  if (event.data.type === "pnl-card-state") {
    elements.resyncButton.disabled = false;
    applyParentState(event.data.payload || {});
  } else if (event.data.type === "pnl-card-media-data") {
    applyMediaData(event.data.payload || {});
  } else if (event.data.type === "pnl-card-media-saved") {
    applyMediaSaved(event.data.payload || {});
  } else if (event.data.type === "pnl-card-action-error") {
    const payload = event.data.payload || {};
    const message = payload?.message || "Action failed.";
    elements.resyncButton.disabled = false;
    setNotice(message);
  } else if (event.data.type === "pnl-card-pause-media") {
    stopPreviewMedia();
  } else if (event.data.type === "pnl-card-download-complete") {
    setNotice("Saved PnL card.");
  } else if (event.data.type === "pnl-card-download-canceled") {
    setNotice("Save canceled.");
  }
}

function emit(type, payload = {}) {
  const message = {
    channel: CHANNEL_OUT,
    type,
    payload
  };
  if (parentPort) {
    parentPort.postMessage(message);
    return;
  }
  window.parent.postMessage(message, PARENT_ORIGIN || "*");
}

function applyParentState(payload) {
  const previousMint = String(state.tokenContext?.mint || "").trim();
  if (Object.prototype.hasOwnProperty.call(payload, "tokenContext")) {
    state.tokenContext = payload.tokenContext || null;
  }
  const nextMint = String(state.tokenContext?.mint || "").trim();
  if (previousMint && nextMint && previousMint !== nextMint) {
    state.walletStatus = null;
  }
  if (Object.prototype.hasOwnProperty.call(payload, "walletStatus")) {
    state.walletStatus = payload.walletStatus || null;
  }
  if (Object.prototype.hasOwnProperty.call(payload, "cardState")) {
    state.cardState = payload.cardState || null;
  }
  if (Object.prototype.hasOwnProperty.call(payload, "solUsd")) {
    state.solUsd = payload.solUsd || null;
  }
  if (Object.prototype.hasOwnProperty.call(payload, "downloadToken")) {
    state.downloadToken = String(payload.downloadToken || "");
  }
  if (Object.prototype.hasOwnProperty.call(payload, "pnlMode")) {
    state.pnlMode = normalizePnlMode(payload.pnlMode);
  }
  const incoming = normalizeSettings(
    payload.settings && typeof payload.settings === "object" ? payload.settings : null
  );
  const localVideoExportFps = state.settings.videoExportFps;
  const localVideoExportSize = state.settings.videoExportSize;
  const localVideoSectionOffset = state.settings.videoSectionOffset;
  const localVolume = state.settings.volume;
  const localMuted = state.settings.muted;
  if (state.localSettingsDirty) {
    const matches = incoming.templateId === state.settings.templateId
      && incoming.mediaId === state.settings.mediaId
      && incoming.videoExportFps === state.settings.videoExportFps
      && incoming.videoExportSize === state.settings.videoExportSize
      && incoming.videoSectionOffset === state.settings.videoSectionOffset;
    if (matches) {
      state.localSettingsDirty = false;
      state.settings = incoming;
    } else {
      state.settings = {
        ...incoming,
        templateId: state.settings.templateId,
        mediaId: state.settings.mediaId,
        videoExportFps: localVideoExportFps,
        videoExportSize: localVideoExportSize,
        videoSectionOffset: localVideoSectionOffset
      };
    }
  } else {
    state.settings = incoming;
  }
  if (state.localAudioSettingsDirty) {
    const audioMatches = Math.abs(incoming.volume - localVolume) < 0.001
      && incoming.muted === localMuted;
    if (audioMatches) {
      state.localAudioSettingsDirty = false;
    } else {
      state.settings = {
        ...state.settings,
        volume: localVolume,
        muted: localMuted
      };
    }
  }
  render();
}

function normalizeSettings(value) {
  const profileHandle = state.cardState?.profile?.handle || "";
  const defaults = state.cardState?.settings?.defaults || {};
  const hasIncomingSettings = value && typeof value === "object";
  const merged = {
    ...DEFAULT_SETTINGS,
    handle: profileHandle || DEFAULT_SETTINGS.handle,
    ...(defaults && typeof defaults === "object" ? defaults : {}),
    ...(hasIncomingSettings ? value : {})
  };
  return sanitizeSettings(merged);
}

function sanitizeSettings(value) {
  const settings = value && typeof value === "object" ? value : {};
  return {
    ...DEFAULT_SETTINGS,
    templateId: String(settings.templateId || DEFAULT_SETTINGS.templateId),
    mediaId: String(settings.mediaId || ""),
    displayCurrency: settings.displayCurrency === "SOL" ? "SOL" : "USD",
    handle: normalizeHandle(settings.handle),
    mainTextColor: normalizeColor(settings.mainTextColor, DEFAULT_SETTINGS.mainTextColor),
    positivePnlColor: normalizeColor(settings.positivePnlColor, DEFAULT_SETTINGS.positivePnlColor),
    negativePnlColor: normalizeColor(settings.negativePnlColor, DEFAULT_SETTINGS.negativePnlColor),
    rectangleTextColor: normalizeColor(settings.rectangleTextColor, DEFAULT_SETTINGS.rectangleTextColor),
    bold: Boolean(settings.bold),
    shadow: Boolean(settings.shadow),
    rectangle: settings.rectangle !== false,
    volume: clampVolume(settings.volume, DEFAULT_SETTINGS.volume),
    muted: Boolean(settings.muted),
    videoExportFps: normalizeVideoExportFps(settings.videoExportFps),
    videoExportSize: normalizeVideoExportSize(settings.videoExportSize).id,
    videoSectionOffset: normalizeVideoSectionOffset(settings.videoSectionOffset)
  };
}

function normalizeVideoExportFps(value) {
  return Number(value) === 60 ? 60 : 30;
}

function normalizePnlMode(value) {
  return value === "gross" ? "gross" : "net";
}

function normalizeVideoExportSize(value) {
  const id = String(value || "").trim();
  return VIDEO_EXPORT_SIZES.find((size) => size.id === id) || VIDEO_EXPORT_SIZES[0];
}

function normalizeVideoSectionOffset(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return 0;
  return Math.max(-100, Math.min(100, Math.round(number)));
}

function clampVolume(value, fallback) {
  const number = Number(value);
  if (!Number.isFinite(number)) return fallback;
  return Math.min(1, Math.max(0, number));
}

function mediaItems() {
  const uploaded = Array.isArray(state.cardState?.media)
    ? state.cardState.media.map((item) => ({
        id: item.id,
        kind: item.kind || "image",
        name: item.name || "Uploaded",
        url: state.mediaDataUrls.get(item.id) || item.dataUrl || "",
        thumbnailUrl: item.thumbnailDataUrl || state.mediaDataUrls.get(item.id) || item.dataUrl || "",
        uploaded: true,
        createdAtUnixMs: Number(item.createdAtUnixMs) || 0
      }))
    : [];
  uploaded.sort((a, b) => b.createdAtUnixMs - a.createdAtUnixMs);
  return [...BUILT_IN_TEMPLATES, ...uploaded];
}

function actualMediaUrl(url) {
  return url || "";
}

function selectedMedia() {
  const items = mediaItems().filter((item) => item.kind === state.mediaMode);
  return items.find((item) => item.id === state.settings.mediaId)
    || items.find((item) => item.id === state.settings.templateId)
    || items[0]
    || BUILT_IN_TEMPLATES[0];
}

function render() {
  document.documentElement.style.setProperty("--text", state.settings.mainTextColor);
  document.documentElement.style.setProperty("--positive", state.settings.positivePnlColor);
  document.documentElement.style.setProperty("--negative", state.settings.negativePnlColor);
  document.documentElement.style.setProperty("--rect-text", state.settings.rectangleTextColor);

  const pnl = currentPnlValue();
  const isNegative = pnl < 0;
  elements.preview.classList.toggle("is-negative", isNegative);
  elements.preview.classList.toggle("is-positive", !isNegative);
  elements.preview.classList.toggle("no-rectangle", !state.settings.rectangle);
  elements.preview.classList.toggle("use-shadow", Boolean(state.settings.shadow));
  elements.preview.classList.toggle("use-bold", Boolean(state.settings.bold));
  elements.preview.classList.toggle("video-preview", state.mediaMode === "video");

  const media = selectedMedia();
  requestMediaData(media);
  setPreviewMedia(media);
  elements.tokenName.textContent = tokenName();
  const isUsdMode = state.settings.displayCurrency === "USD";
  elements.pnlCurrencyIcon.classList.toggle("hidden", isUsdMode);
  elements.pnlCurrencySymbol.classList.add("hidden");
  if (!isUsdMode) {
    const pnlColor = isNegative ? state.settings.negativePnlColor : state.settings.positivePnlColor;
    const pnlSymbolColor = state.settings.rectangle ? state.settings.rectangleTextColor : pnlColor;
    elements.pnlCurrencyIcon.src = solIconSvg(pnlSymbolColor);
  }
  elements.pnlAmountText.textContent = formatSignedMoneyText(pnl, currentPnlValueKind());
  elements.pnlPercent.textContent = formatPercent(currentPnlPercent());
  renderMetricValue(elements.investedAmount, currentInvestedValue(), currentInvestedValueKind());
  renderMetricValue(elements.positionAmount, currentPositionValue(), currentPositionValueKind());
  elements.handle.textContent = state.settings.handle || "@";
  elements.siteLabel.textContent = "Trench.Tools";
  elements.promoLineOne.textContent = PROMO_LINE_ONE;
  elements.promoLineTwo.textContent = PROMO_LINE_TWO;
  elements.currencyButton.textContent = state.settings.displayCurrency;
  elements.currencyButton.title = currencyButtonTitle();
  elements.modeImage.classList.toggle("active", state.mediaMode === "image");
  elements.modeVideo.classList.toggle("active", state.mediaMode === "video");
  const isVideoMode = state.mediaMode === "video";
  elements.volumeControl.classList.toggle("hidden", !isVideoMode);
  elements.fpsButton.classList.toggle("hidden", !isVideoMode);
  elements.fpsButton.disabled = Boolean(state.exporting);
  elements.fpsButton.textContent = `${exportFps()} FPS`;
  elements.videoSizeButton.classList.toggle("hidden", !isVideoMode);
  elements.videoSizeButton.disabled = Boolean(state.exporting);
  elements.videoSizeButton.textContent = videoExportSize().label;
  syncVideoSectionOffsetStyle();
  elements.copyButton.classList.toggle("hidden", isVideoMode);
  elements.copyButton.disabled = Boolean(state.exporting) || isVideoMode;
  elements.downloadButton.disabled = Boolean(state.exporting);
  elements.downloadButton.textContent = exportButtonText();
  syncVolumeUi();
  applyVideoAudioState();
  renderMediaStrip();
  scheduleResize();
}

function exportButtonText() {
  if (state.exporting) {
    const progress = Math.max(0, Math.min(99, Math.floor(Number(state.exportProgress) || 0)));
    return `Exporting ${progress}%`;
  }
  return state.mediaMode === "video" ? "Export" : "Download";
}

function exportFps() {
  return normalizeVideoExportFps(state.settings.videoExportFps);
}

function videoExportSize() {
  return normalizeVideoExportSize(state.settings.videoExportSize);
}

function videoSectionOffset() {
  return normalizeVideoSectionOffset(state.settings.videoSectionOffset);
}

function syncVideoSectionOffsetStyle() {
  if (state.mediaMode !== "video") {
    elements.preview.style.setProperty("--video-section-offset-y", "0px");
    elements.preview.style.removeProperty("--video-source-aspect");
    return;
  }
  elements.preview.style.setProperty("--video-source-aspect", `${state.previewVideoAspect}`);
  const offsetPx = videoSectionRangePx() * (videoSectionOffset() / 100);
  elements.preview.style.setProperty("--video-section-offset-y", `${offsetPx}px`);
}

function videoSectionRangePx() {
  const previewWidth = Math.max(1, elements.preview.clientWidth || elements.preview.getBoundingClientRect().width || CARD_WIDTH);
  const previewHeight = Math.max(1, elements.preview.clientHeight || elements.preview.getBoundingClientRect().height || CARD_HEIGHT);
  const sectionHeight = previewWidth * (9 / 16);
  return Math.max(0, previewHeight - sectionHeight) / 2;
}

function applyVideoAudioState() {
  const video = elements.mediaVideo;
  if (!video) return;
  const volume = clampVolume(state.settings.volume, DEFAULT_SETTINGS.volume);
  video.volume = volume;
  video.muted = Boolean(state.settings.muted) || volume === 0;
}

function tryPlayPreviewVideo() {
  playPreviewVideoElement(elements.mediaVideo);
  if (state.mediaMode === "video") {
    playPreviewVideoElement(elements.previewBackgroundVideo);
  }
}

function playPreviewVideoElement(video) {
  if (!video || !video.src) return;
  const playPromise = video.play();
  if (!playPromise || typeof playPromise.catch !== "function") return;
  playPromise.catch(() => {
    if (video.muted) return;
    video.muted = true;
    video.play().catch(() => {});
  });
}

function pausePreviewMedia() {
  for (const video of [elements.mediaVideo, elements.previewBackgroundVideo]) {
    if (!video) continue;
    try {
      video.pause();
      if (!video.muted) video.muted = true;
    } catch (_error) {}
  }
}

function stopPreviewMedia() {
  for (const video of [elements.mediaVideo, elements.previewBackgroundVideo]) {
    if (!video) continue;
    try {
      video.pause();
      video.muted = true;
      video.removeAttribute("src");
      video.load();
    } catch (_error) {}
  }
  state.previewVideoUrl = "";
  state.previewBackgroundVideoUrl = "";
}

function syncVolumeUi() {
  const volume = clampVolume(state.settings.volume, DEFAULT_SETTINGS.volume);
  const muted = Boolean(state.settings.muted) || volume === 0;
  if (Number(elements.volumeSlider.value) !== Math.round(volume * 100)) {
    elements.volumeSlider.value = String(Math.round(volume * 100));
  }
  elements.volumeIcon.textContent = muted ? "🔇" : volume < 0.34 ? "🔈" : volume < 0.67 ? "🔉" : "🔊";
  elements.volumeButton.setAttribute("aria-label", muted ? "Unmute video" : "Mute video");
  elements.volumeButton.title = muted ? "Unmute" : "Mute";
}

function renderMediaStrip() {
  const items = mediaItems().filter((item) => item.kind === state.mediaMode);
  elements.mediaStrip.innerHTML = "";
  for (const item of items) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `media-tile${selectedMedia().id === item.id ? " active" : ""}`;
    button.title = item.name;
    const preview = item.kind === "video" && !item.thumbnailUrl
      ? document.createElement("video")
      : document.createElement("img");
    preview.src = actualMediaUrl(item.thumbnailUrl || item.url);
    preview.alt = item.kind === "video" ? "" : item.name;
    if (preview instanceof HTMLVideoElement) {
      preview.muted = true;
      preview.loop = true;
      preview.playsInline = true;
      preview.preload = "metadata";
    }
    button.appendChild(preview);
    if (item.kind === "video") {
      const badge = document.createElement("span");
      badge.className = "media-video-badge";
      badge.setAttribute("aria-hidden", "true");
      badge.textContent = "▶";
      button.appendChild(badge);
    }
    button.addEventListener("click", () => {
      if (item.uploaded) {
        if (state.settings.mediaId === item.id) return;
        state.settings.mediaId = item.id;
        state.settings.templateId = "";
      } else {
        if (state.settings.templateId === item.id && !state.settings.mediaId) return;
        state.settings.templateId = item.id;
        state.settings.mediaId = "";
      }
      state.localSettingsDirty = true;
      persistSettings();
      render();
    });
    if (item.uploaded) {
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "media-remove";
      remove.title = "Remove";
      remove.setAttribute("aria-label", `Remove ${item.name}`);
      remove.textContent = "×";
      remove.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        deleteUploadedMedia(item.id);
      });
      button.appendChild(remove);
    }
    elements.mediaStrip.appendChild(button);
  }
}

function deleteUploadedMedia(mediaId) {
  const id = String(mediaId || "").trim();
  if (!id) return;
  state.mediaDataUrls.delete(id);
  state.pendingMediaData.delete(id);
  if (state.settings.mediaId === id) {
    state.settings.mediaId = "";
    state.settings.templateId = BUILT_IN_TEMPLATES[0]?.id || "";
    persistSettings();
  }
  emit("pnl-card-delete-media", { mediaId: id });
}

function tokenName() {
  return state.tokenContext?.symbol
    || state.tokenContext?.name
    || "Token";
}

function currentHeldBalance() {
  return Number(
    state.walletStatus?.holdingAmount
    ?? state.walletStatus?.mintBalanceUi
    ?? state.walletStatus?.tokenBalance
    ?? 0
  ) || 0;
}

function currentPositionValue() {
  if (historicalUsdAvailable()) {
    return Number(state.walletStatus?.positionValueUsdMixed) || 0;
  }
  return (Number(state.walletStatus?.trackedSoldSol) || 0) + (Number(state.walletStatus?.holdingValueSol) || 0);
}

function currentPositionValueKind() {
  return historicalUsdAvailable() ? "USD" : "SOL";
}

function currentInvestedValue() {
  if (historicalUsdAvailable()) {
    return Number(state.walletStatus?.trackedBoughtUsdHistorical) || 0;
  }
  return Number(state.walletStatus?.trackedBoughtSol) || 0;
}

function currentInvestedValueKind() {
  return historicalUsdAvailable() ? "USD" : "SOL";
}

function currentPnlValue() {
  if (historicalUsdAvailable()) {
    const usdKey = state.pnlMode === "gross" ? "pnlGrossUsdHistorical" : "pnlNetUsdHistorical";
    return Number(state.walletStatus?.[usdKey]) || 0;
  }
  const key = state.pnlMode === "gross" ? "pnlGross" : "pnlNet";
  return Number(state.walletStatus?.[key]) || 0;
}

function currentPnlValueKind() {
  return historicalUsdAvailable() ? "USD" : "SOL";
}

function currentPnlPercent() {
  if (historicalUsdAvailable()) {
    const invested = Number(state.walletStatus?.trackedBoughtUsdHistorical) || 0;
    if (invested > 0) {
      return (currentPnlValue() / invested) * 100;
    }
  }
  const key = state.pnlMode === "gross" ? "pnlPercentGross" : "pnlPercentNet";
  return Number(state.walletStatus?.[key]) || 0;
}

function convertValue(value, kind = "SOL") {
  const number = Number(value) || 0;
  if (kind === "USD") return number;
  const sol = number;
  if (state.settings.displayCurrency !== "USD") return sol;
  const price = Number(state.solUsd?.price);
  return Number.isFinite(price) && price > 0 ? sol * price : null;
}

function formatMoney(value, kind = "SOL") {
  const converted = convertValue(value, kind);
  if (converted == null) {
    return "--";
  }
  if (state.settings.displayCurrency === "SOL" && kind !== "USD") {
    return `${formatSolValueText(converted)} SOL`;
  }
  if (state.settings.displayCurrency === "SOL") {
    return `${formatUsdValueText(converted)}`;
  }
  return `$${formatUsdValueText(converted)}`;
}

function renderMetricValue(element, value, kind = "SOL") {
  if (!(element instanceof HTMLElement)) return;
  element.replaceChildren();
  const converted = convertValue(value, kind);
  if (converted == null) {
    element.textContent = "--";
    return;
  }
  if (state.settings.displayCurrency === "SOL" && kind !== "USD") {
    const icon = document.createElement("img");
    icon.src = solIconSvg(state.settings.mainTextColor);
    icon.alt = "";
    icon.className = "metric-icon";
    element.appendChild(icon);
    element.appendChild(document.createTextNode(formatSolValueText(converted)));
  } else {
    element.textContent = `$${formatUsdValueText(converted)}`;
  }
}

function formatSignedMoneyText(value, kind = "SOL") {
  const converted = convertValue(value, kind);
  if (converted == null) {
    return "--";
  }
  const sign = converted >= 0 ? "+" : "-";
  const formatter = state.settings.displayCurrency === "SOL" && kind !== "USD"
    ? formatSolValueText
    : formatUsdValueText;
  const prefix = state.settings.displayCurrency === "USD" || kind === "USD" ? "$" : "";
  return `${sign}${prefix}${formatter(Math.abs(converted))}`;
}

function formatNumberText(value, decimals) {
  const number = Number(value) || 0;
  return Math.abs(number).toFixed(decimals).replace(/\.?0+$/, "");
}

function formatSolValueText(value) {
  return formatCompactMoneyText(value);
}

function formatUsdValueText(value) {
  return formatCompactMoneyText(value);
}

function formatCompactMoneyText(value) {
  const number = Math.abs(Number(value) || 0);
  if (number >= 1000) {
    const thousands = number / 1000;
    const decimals = thousands < 10 ? 1 : 0;
    return `${formatNumberText(thousands, decimals)}k`;
  }
  if (number >= 100) return formatNumberText(number, 1);
  if (number >= 10) return formatNumberText(number, 2);
  if (number >= 1) return formatNumberText(number, 3);
  if (number > 0) return formatNumberText(number, 3);
  return "0";
}


function formatPercent(value) {
  const number = Number(value) || 0;
  return `${number >= 0 ? "+" : ""}${number.toFixed(2)}%`;
}

function setMediaMode(mode) {
  const nextMode = mode === "video" ? "video" : "image";
  if (state.mediaMode === nextMode) return;
  const previousRect = elements.preview.getBoundingClientRect();
  state.mediaMode = nextMode;
  render();
  animatePreviewModeChange(previousRect);
}

function animatePreviewModeChange(previousRect) {
  const nextRect = elements.preview.getBoundingClientRect();
  if (!previousRect || !nextRect || typeof elements.preview.animate !== "function") {
    return;
  }
  const widthDelta = Math.abs(previousRect.width - nextRect.width);
  const heightDelta = Math.abs(previousRect.height - nextRect.height);
  if (widthDelta < 1 && heightDelta < 1) {
    return;
  }
  elements.preview.animate(
    [
      {
        width: `${previousRect.width}px`,
        height: `${previousRect.height}px`,
        opacity: 0.88,
        transform: "scale(0.992)"
      },
      {
        width: `${nextRect.width}px`,
        height: `${nextRect.height}px`,
        opacity: 1,
        transform: "scale(1)"
      }
    ],
    {
      duration: 320,
      easing: "cubic-bezier(0.22, 1, 0.36, 1)"
    }
  );
}

function syncCustomizeInputs() {
  elements.handleInput.value = (state.settings.handle || "").replace(/^@+/, "");
  elements.mainColor.value = state.settings.mainTextColor;
  elements.positiveColor.value = state.settings.positivePnlColor;
  elements.negativeColor.value = state.settings.negativePnlColor;
  elements.rectangleTextColor.value = state.settings.rectangleTextColor;
  elements.boldToggle.checked = Boolean(state.settings.bold);
  elements.shadowToggle.checked = Boolean(state.settings.shadow);
  elements.rectangleToggle.checked = Boolean(state.settings.rectangle);
  syncAllColorHexes();
}

function collectCustomizeInputs() {
  state.settings.handle = normalizeHandle(elements.handleInput.value);
  state.settings.mainTextColor = elements.mainColor.value;
  state.settings.positivePnlColor = elements.positiveColor.value;
  state.settings.negativePnlColor = elements.negativeColor.value;
  state.settings.rectangleTextColor = elements.rectangleTextColor.value;
  state.settings.bold = elements.boldToggle.checked;
  state.settings.shadow = elements.shadowToggle.checked;
  state.settings.rectangle = elements.rectangleToggle.checked;
}

function normalizeHandle(value) {
  const trimmed = String(value || "").trim().replace(/^@+/, "");
  return trimmed ? `@${trimmed.slice(0, 32)}` : "";
}

function normalizeColor(value, fallback) {
  const color = String(value || "").trim();
  return /^#[0-9a-f]{6}$/i.test(color) ? color : fallback;
}

function usdPriceAvailable() {
  if (state.settings.displayCurrency !== "USD") return true;
  if (historicalUsdAvailable()) return true;
  const price = Number(state.solUsd?.price);
  return Number.isFinite(price) && price > 0;
}

function historicalUsdAvailable() {
  if (state.settings.displayCurrency !== "USD") return false;
  const walletStatus = state.walletStatus || {};
  return walletStatus.usdPriceMode === "historical-daily"
    && walletStatus.usdHistoricalCoverage === "complete"
    && Number.isFinite(Number(walletStatus.trackedBoughtUsdHistorical))
    && Number.isFinite(Number(walletStatus.positionValueUsdMixed));
}

function currencyButtonTitle() {
  if (state.settings.displayCurrency !== "USD") {
    return "Toggle SOL/USD";
  }
  if (historicalUsdAvailable()) {
    return "USD uses historical daily SOL closes for buys/sells and current SOL/USD for holdings";
  }
  return usdPriceAvailable() ? "USD est. current SOL price" : "SOL/USD price unavailable";
}

function persistProfile() {
  emit("pnl-card-save-profile", { handle: state.settings.handle || "" });
}

function persistSettings() {
  emit("pnl-card-save-settings", { defaults: sanitizeSettings(state.settings), overrides: {} });
}

async function uploadMedia(file) {
  try {
    const isImage = SUPPORTED_IMAGE_TYPES.has(file.type);
    const isVideo = SUPPORTED_VIDEO_TYPES.has(file.type);
    if (!isImage && !isVideo) {
      setNotice("Unsupported file type. Use PNG, JPG, WebP, MP4, or WebM.");
      return;
    }
    const sizeCap = isVideo ? MAX_VIDEO_BYTES : MAX_IMAGE_BYTES;
    if (file.size > sizeCap) {
      setNotice(
        `File is too large (${formatBytes(file.size)}). Max ${formatBytes(sizeCap)}.`
      );
      return;
    }
    setNotice("Uploading media...");
    const dataUrl = await readFileAsDataUrl(file);
    let thumbnailDataUrl = dataUrl;
    if (isVideo) {
      const { duration, thumbnail } = await analyzeVideoForUpload(dataUrl);
      if (!Number.isFinite(duration) || duration <= 0) {
        setNotice("Could not read video duration. Try a different file.");
        return;
      }
      if (duration > MAX_VIDEO_DURATION_SECONDS + 0.05) {
        setNotice(
          `Video is too long (${duration.toFixed(1)}s). Max ${MAX_VIDEO_DURATION_SECONDS}s.`
        );
        return;
      }
      thumbnailDataUrl = thumbnail || "";
    }
    const payload = {
      name: file.name,
      contentType: file.type,
      dataUrl,
      thumbnailDataUrl
    };
    emit("pnl-card-save-media", payload);
  } catch (error) {
    setNotice(error?.message || "Media upload failed.");
  }
}

function formatBytes(bytes) {
  const n = Number(bytes) || 0;
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${n} B`;
}

async function analyzeVideoForUpload(dataUrl) {
  const video = document.createElement("video");
  video.muted = true;
  video.playsInline = true;
  video.preload = "auto";
  video.crossOrigin = "anonymous";
  video.src = dataUrl;
  try {
    await waitForVideoMetadata(video);
    const duration = Number(video.duration) || 0;
    if (Number.isFinite(duration) && duration > MAX_VIDEO_DURATION_SECONDS + 0.05) {
      return { duration, thumbnail: "" };
    }
    const seekTarget = Math.min(VIDEO_THUMBNAIL_SEEK_SECONDS, Math.max(0, duration - 0.05));
    if (seekTarget > 0) {
      try {
        await seekVideo(video, seekTarget);
      } catch (_error) {}
    } else {
      try {
        await ensureVideoFrame(video);
      } catch (_error) {}
    }
    const thumbnail = renderVideoFrameToDataUrl(video);
    return { duration, thumbnail };
  } finally {
    try {
      video.removeAttribute("src");
      video.load();
    } catch (_error) {}
  }
}

function waitForVideoMetadata(video) {
  return new Promise((resolve, reject) => {
    if (video.readyState >= HTMLMediaElement.HAVE_METADATA) {
      resolve();
      return;
    }
    const timeout = window.setTimeout(() => {
      cleanup();
      reject(new Error("Unable to read video metadata."));
    }, 8000);
    const cleanup = () => {
      window.clearTimeout(timeout);
      video.removeEventListener("loadedmetadata", onMeta);
      video.removeEventListener("error", onError);
    };
    const onMeta = () => {
      cleanup();
      resolve();
    };
    const onError = () => {
      cleanup();
      reject(new Error("Unable to read video metadata."));
    };
    video.addEventListener("loadedmetadata", onMeta, { once: true });
    video.addEventListener("error", onError, { once: true });
    video.load();
  });
}

function seekVideo(video, seconds) {
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      cleanup();
      reject(new Error("Video seek timeout."));
    }, 4000);
    const cleanup = () => {
      window.clearTimeout(timeout);
      video.removeEventListener("seeked", onSeeked);
      video.removeEventListener("error", onError);
    };
    const onSeeked = () => {
      cleanup();
      resolve();
    };
    const onError = () => {
      cleanup();
      reject(new Error("Video seek failed."));
    };
    video.addEventListener("seeked", onSeeked, { once: true });
    video.addEventListener("error", onError, { once: true });
    try {
      video.currentTime = Math.max(0, Number(seconds) || 0);
    } catch (error) {
      cleanup();
      reject(error);
    }
  });
}

function renderVideoFrameToDataUrl(video) {
  const width = Math.max(1, video.videoWidth || 820);
  const height = Math.max(1, video.videoHeight || 460);
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  try {
    ctx.drawImage(video, 0, 0, width, height);
  } catch (_error) {
    return "";
  }
  try {
    return canvas.toDataURL("image/jpeg", 0.82);
  } catch (_error) {
    return "";
  }
}

function readFileAsDataUrl(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ""));
    reader.onerror = () => reject(reader.error || new Error("Failed to read file."));
    reader.readAsDataURL(file);
  });
}

function blobToDataUrl(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ""));
    reader.onerror = () => reject(reader.error || new Error("Failed to prepare download."));
    reader.readAsDataURL(blob);
  });
}

function setExportProgress(progress) {
  state.exportProgress = Math.max(0, Math.min(99, Number(progress) || 0));
  elements.downloadButton.textContent = exportButtonText();
}

async function exportCard(mode) {
  if (state.exporting) {
    return;
  }
  const isVideoDownload = mode === "export" && selectedMedia().kind === "video";
  try {
    state.exporting = true;
    state.exportProgress = 0;
    render();
    setNotice(mode === "copy" ? "Copying..." : isVideoDownload ? "Preparing export..." : "Preparing download...");
    const blob = isVideoDownload ? null : await renderCardBlob();
    if (mode === "copy") {
      await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
      setNotice("Copied PnL card.");
      return;
    }
    const download = isVideoDownload
      ? await renderCardVideoDownload(setExportProgress)
      : await buildImageDownloadPayload(blob);
    if (isVideoDownload) {
      setExportProgress(99);
    }
    emit("pnl-card-download", {
      filename: download.filename,
      downloadToken: state.downloadToken,
      dataUrl: download.dataUrl
    });
  } catch (error) {
    setNotice(error?.message || "Export failed.");
  } finally {
    state.exporting = false;
    state.exportProgress = 0;
    render();
  }
}

async function buildImageDownloadPayload(renderedImageBlob) {
  const baseName = `${tokenName().replace(/[^a-z0-9_-]+/gi, "-") || "token"}-pnl`;
  return {
    filename: `${baseName}.png`,
    dataUrl: await blobToDataUrl(renderedImageBlob)
  };
}

async function renderCardVideoDownload(onProgress = null) {
  const mimeType = supportedVideoExportMimeType();
  if (!mimeType) {
    throw new Error("Video export is not supported in this browser.");
  }
  const media = await ensureSelectedMediaData();
  setPreviewMedia(media);
  const video = elements.mediaVideo;
  if (!video?.src) {
    throw new Error("Still loading uploaded video.");
  }
  await ensureFontsReady();
  await waitForVideoMetadata(video);
  const duration = Number.isFinite(video.duration) && video.duration > 0
    ? Math.min(video.duration, MAX_VIDEO_DURATION_SECONDS)
    : MAX_VIDEO_DURATION_SECONDS;
  const exportSize = videoExportSize();
  const canvas = document.createElement("canvas");
  canvas.width = exportSize.width;
  canvas.height = exportSize.height;
  const ctx = canvas.getContext("2d");
  const layout = exportLayout();
  const renderer = await createVideoExportRenderer(ctx, layout, exportSize);
  const fps = exportFps();
  const stream = buildVideoExportStream(canvas, video, fps);
  const recorder = new MediaRecorder(stream, { mimeType });
  const chunks = [];
  recorder.addEventListener("dataavailable", (event) => {
    if (event.data?.size > 0) chunks.push(event.data);
  });
  const stopped = new Promise((resolve, reject) => {
    recorder.addEventListener("stop", resolve, { once: true });
    recorder.addEventListener("error", () => reject(new Error("Video export failed.")), { once: true });
  });
  const restore = snapshotVideoPlayback(video);
  try {
    if (video.currentTime > 0.02) {
      await seekVideo(video, 0);
    }
    applyVideoAudioState();
    await video.play();
    recorder.start();
    await recordComposedVideoFrames(video, renderer, duration, fps, onProgress);
    recorder.stop();
    await stopped;
  } finally {
    restore();
  }
  const downloadMimeType = downloadVideoMimeType(mimeType);
  const blob = new Blob(chunks, { type: downloadMimeType });
  if (!blob.size) {
    throw new Error("Video export produced no media data.");
  }
  const baseName = `${tokenName().replace(/[^a-z0-9_-]+/gi, "-") || "token"}-pnl`;
  const dataUrl = await blobToDataUrl(blob);
  const extension = dataUrlMimeType(dataUrl) === "video/mp4" ? "mp4" : "webm";
  return {
    filename: `${baseName}.${extension}`,
    dataUrl
  };
}

function buildVideoExportStream(canvas, video, fps = exportFps()) {
  const stream = canvas.captureStream(fps);
  if (state.settings.muted || clampVolume(state.settings.volume, DEFAULT_SETTINGS.volume) <= 0) {
    return stream;
  }
  const mediaStream = typeof video.captureStream === "function"
    ? video.captureStream()
    : typeof video.mozCaptureStream === "function"
      ? video.mozCaptureStream()
      : null;
  mediaStream?.getAudioTracks?.().forEach((track) => {
    stream.addTrack(track);
  });
  return stream;
}

function supportedVideoExportMimeType() {
  if (typeof MediaRecorder === "undefined") return "";
  return VIDEO_EXPORT_MIME_TYPES.find((type) => MediaRecorder.isTypeSupported(type)) || "";
}

function downloadVideoMimeType(recordingMimeType) {
  const type = String(recordingMimeType || "").toLowerCase();
  return type.startsWith("video/mp4") ? "video/mp4" : "video/webm";
}

function snapshotVideoPlayback(video) {
  const currentTime = Number(video.currentTime) || 0;
  const muted = Boolean(video.muted);
  const paused = Boolean(video.paused);
  const loop = Boolean(video.loop);
  return () => {
    try {
      video.pause();
      video.currentTime = Math.min(currentTime, Number(video.duration) || currentTime);
      video.muted = muted;
      video.loop = loop;
      if (!paused) void video.play();
    } catch (_error) {}
  };
}

async function recordComposedVideoFrames(video, renderFrame, durationSeconds, fps = exportFps(), onProgress = null) {
  const startedAt = performance.now();
  const frameIntervalMs = 1000 / fps;
  let nextFrameAt = startedAt;
  let lastProgress = -1;
  while (performance.now() - startedAt < durationSeconds * 1000) {
    renderFrame(video);
    if (typeof onProgress === "function") {
      const elapsedSeconds = Math.min(durationSeconds, (performance.now() - startedAt) / 1000);
      const progress = Math.min(98, Math.floor((elapsedSeconds / durationSeconds) * 98));
      if (progress !== lastProgress) {
        lastProgress = progress;
        onProgress(progress);
      }
    }
    nextFrameAt += frameIntervalMs;
    await sleep(Math.max(0, nextFrameAt - performance.now()));
  }
  renderFrame(video);
  if (typeof onProgress === "function") {
    onProgress(98);
  }
}

function sleep(ms) {
  return new Promise((resolve) => window.setTimeout(resolve, Math.max(0, ms)));
}

function dataUrlMimeType(dataUrl) {
  const match = /^data:([^;,]+)[;,]/i.exec(String(dataUrl || ""));
  return match ? match[1].toLowerCase() : "";
}

async function renderCardBlob() {
  await ensureFontsReady();
  const canvas = document.createElement("canvas");
  canvas.width = CARD_WIDTH;
  canvas.height = CARD_HEIGHT;
  const ctx = canvas.getContext("2d");
  const layout = exportLayout();
  const media = await exportMediaElement();
  const renderer = await createCardRenderer(ctx, layout);
  renderer(media);

  return await new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob);
      else reject(new Error("Unable to render card."));
    }, "image/png");
  });
}

async function createCardRenderer(ctx, layout) {
  const fontStack = `"Inter", "SF Pro Display", "Segoe UI Variable Display", "Segoe UI", -apple-system, BlinkMacSystemFont, system-ui, sans-serif`;
  const DEFAULT_TEXT_SHADOW = { color: "rgba(0,0,0,0.6)", blur: 6, offsetY: 1 };
  const drawText = (text, x, y, size, options = {}) => {
    const style = options.style || "normal";
    const weight = options.weight || 500;
    ctx.font = `${style} ${weight} ${Math.round(size)}px ${fontStack}`;
    ctx.fillStyle = options.color || state.settings.mainTextColor;
    ctx.textAlign = options.align || "left";
    ctx.textBaseline = "top";
    if (options.tracking != null) {
      ctx.letterSpacing = `${options.tracking}px`;
    } else {
      ctx.letterSpacing = "0px";
    }
    const shadow = options.shadow === false
      ? null
      : options.shadow || (state.settings.shadow
        ? { color: "rgba(0,0,0,0.9)", blur: 10, offsetY: 2 }
        : DEFAULT_TEXT_SHADOW);
    if (shadow) {
      ctx.shadowColor = shadow.color || "rgba(0,0,0,0.6)";
      ctx.shadowBlur = shadow.blur ?? 6;
      ctx.shadowOffsetY = shadow.offsetY ?? 1;
    } else {
      ctx.shadowColor = "transparent";
      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;
    }
    ctx.fillText(text, x, y);
    ctx.shadowColor = "transparent";
    ctx.shadowBlur = 0;
    ctx.shadowOffsetY = 0;
    ctx.letterSpacing = "0px";
  };

  let whiteLogo = null;
  try {
    const logo = await loadImage(elements.brandLogo.src);
    whiteLogo = recolorImageToWhite(logo);
  } catch (_error) {}
  const mainColor = state.settings.mainTextColor;
  const softTextColor = applyAlpha(mainColor, 0.85);
  const mutedTextColor = applyAlpha(mainColor, 0.7);

  const pnl = currentPnlValue();
  const pnlColor = pnl < 0 ? state.settings.negativePnlColor : state.settings.positivePnlColor;
  const pnlText = formatSignedMoneyText(pnl, currentPnlValueKind());
  const isUsdMode = state.settings.displayCurrency === "USD";

  const symbolColor = state.settings.rectangle ? state.settings.rectangleTextColor : pnlColor;
  const solIcon = isUsdMode ? null : await loadImage(solIconSvg(symbolColor));
  const metricSolIcon = state.settings.displayCurrency === "SOL"
    ? await loadImage(solIconSvg(mainColor))
    : null;

  return (media) => {
    ctx.clearRect(0, 0, CARD_WIDTH, CARD_HEIGHT);
    drawMediaCover(ctx, media, 0, 0, CARD_WIDTH, CARD_HEIGHT);
    const gradient = ctx.createLinearGradient(0, 0, CARD_WIDTH, 0);
    gradient.addColorStop(0, "rgba(0,0,0,0.74)");
    gradient.addColorStop(0.44, "rgba(0,0,0,0.25)");
    gradient.addColorStop(1, "rgba(0,0,0,0.08)");
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, CARD_WIDTH, CARD_HEIGHT);

    if (whiteLogo) {
      drawMediaContain(ctx, whiteLogo, layout.brand.x, layout.brand.y, layout.brand.width, layout.brand.height);
    }

    drawText("Trench.Tools", layout.siteLabel.right, layout.siteLabel.y, layout.siteLabel.size, {
      weight: layout.siteLabel.weight,
      align: "right",
      color: mainColor,
      tracking: layout.siteLabel.tracking
    });

    drawText(tokenName(), layout.token.x, layout.token.y, layout.token.size, {
      weight: layout.token.weight,
      tracking: layout.token.tracking,
      color: mainColor
    });

  if (state.settings.rectangle) {
    ctx.fillStyle = pnlColor;
    drawRoundedRect(ctx, layout.pnl.x, layout.pnl.y, layout.pnl.width, layout.pnl.height, layout.pnl.radius);
    ctx.fill();
  }

  if (!isUsdMode && solIcon) {
    drawMediaContain(ctx, solIcon, layout.pnlSymbol.x, layout.pnlSymbol.y, layout.pnlSymbol.width, layout.pnlSymbol.height);
  }

  drawText(pnlText, layout.pnlText.x, layout.pnlText.y, layout.pnlText.size, {
    weight: layout.pnlText.weight,
    color: state.settings.rectangle ? state.settings.rectangleTextColor : pnlColor,
    tracking: layout.pnlText.tracking,
    shadow: state.settings.rectangle ? false : undefined
  });

  const labelColor = mutedTextColor;
  const valueColor = mainColor;
  const drawMetricValue = (value, row, color, kind = "SOL") => {
    const finalColor = color || valueColor;
    if (state.settings.displayCurrency === "SOL" && kind !== "USD") {
      const converted = convertValue(value, kind);
      if (converted == null) {
        drawText("--", row.valueRight, row.y, row.size, {
          weight: row.valueWeight,
          align: "right",
          color: finalColor
        });
        return;
      }
      const numberText = formatSolValueText(converted);
      ctx.font = `${row.valueWeight} ${row.size}px ${fontStack}`;
      const textWidth = ctx.measureText(numberText).width;
      const iconX = row.valueRight - textWidth - row.iconGap - row.iconSize;
      const iconY = row.y + (row.size - row.iconSize) / 2;
      if (metricSolIcon) {
        ctx.save();
        ctx.shadowColor = "rgba(0,0,0,0.55)";
        ctx.shadowBlur = 6;
        ctx.shadowOffsetY = 1;
        drawMediaContain(ctx, metricSolIcon, iconX, iconY, row.iconSize, row.iconSize);
        ctx.restore();
      }
      drawText(numberText, row.valueRight, row.y, row.size, {
        weight: row.valueWeight,
        align: "right",
        color: finalColor
      });
    } else {
      drawText(formatMoney(value, kind), row.valueRight, row.y, row.size, {
        weight: row.valueWeight,
        align: "right",
        color: finalColor
      });
    }
  };
  const [pnlRow, investedRow, positionRow] = layout.metrics;
  drawText("PNL", pnlRow.labelX, pnlRow.y, pnlRow.size, { weight: pnlRow.labelWeight, color: labelColor });
  drawText(formatPercent(currentPnlPercent()), pnlRow.valueRight, pnlRow.y, pnlRow.size, {
    weight: pnlRow.valueWeight,
    align: "right",
    color: pnlColor
  });
  drawText("Invested", investedRow.labelX, investedRow.y, investedRow.size, { weight: investedRow.labelWeight, color: labelColor });
  drawMetricValue(currentInvestedValue(), investedRow, undefined, currentInvestedValueKind());
  drawText("Position", positionRow.labelX, positionRow.y, positionRow.size, { weight: positionRow.labelWeight, color: labelColor });
  drawMetricValue(currentPositionValue(), positionRow, undefined, currentPositionValueKind());

  drawText(PROMO_LINE_ONE, layout.promoOne.x, layout.promoOne.y, layout.promoOne.size, {
    weight: layout.promoOne.weight,
    color: softTextColor,
    tracking: layout.promoOne.tracking
  });
  drawText(PROMO_LINE_TWO, layout.promoTwo.x, layout.promoTwo.y, layout.promoTwo.size, {
    weight: layout.promoTwo.weight,
    color: mainColor,
    tracking: layout.promoTwo.tracking
  });

  drawText(state.settings.handle || "@", layout.handle.x, layout.handle.y, layout.handle.size, {
    weight: layout.handle.weight,
    tracking: layout.handle.tracking,
    color: mainColor
  });
  };
}

async function createVideoExportRenderer(ctx, layout, exportSize) {
  if (exportSize.scene !== "16:9") {
    return await createCardRenderer(ctx, layout);
  }
  const cardCanvas = document.createElement("canvas");
  cardCanvas.width = exportSize.width;
  cardCanvas.height = exportSize.height;
  const cardCtx = cardCanvas.getContext("2d");
  const cardLayout = exportLayout();
  const innerCardRenderer = await createCardOverlayRenderer(cardCtx, cardLayout, exportSize);
  return (media) => {
    ctx.clearRect(0, 0, exportSize.width, exportSize.height);
    drawVideoSectionCover(ctx, media, 0, 0, exportSize.width, exportSize.height);
    innerCardRenderer();
    ctx.drawImage(cardCanvas, 0, 0, exportSize.width, exportSize.height);
  };
}

async function createCardOverlayRenderer(ctx, layout, exportSize) {
  const fontStack = `"Inter", "SF Pro Display", "Segoe UI Variable Display", "Segoe UI", -apple-system, BlinkMacSystemFont, system-ui, sans-serif`;
  const drawText = (text, x, y, size, options = {}) => {
    ctx.font = `${options.style || "normal"} ${options.weight || 500} ${Math.round(size)}px ${fontStack}`;
    ctx.fillStyle = options.color || state.settings.mainTextColor;
    ctx.textAlign = options.align || "left";
    ctx.textBaseline = "top";
    if (options.tracking != null) {
      ctx.letterSpacing = `${options.tracking}px`;
    } else {
      ctx.letterSpacing = "0px";
    }
    const shadow = options.shadow === false
      ? null
      : state.settings.shadow
        ? { color: "rgba(0,0,0,0.9)", blur: 10, offsetY: 2 }
        : { color: "rgba(0,0,0,0.6)", blur: 6, offsetY: 1 };
    if (shadow) {
      ctx.shadowColor = shadow.color;
      ctx.shadowBlur = shadow.blur;
      ctx.shadowOffsetY = shadow.offsetY;
    }
    ctx.fillText(text, x, y);
    ctx.shadowColor = "transparent";
    ctx.shadowBlur = 0;
    ctx.shadowOffsetY = 0;
    ctx.letterSpacing = "0px";
  };
  let whiteLogo = null;
  try {
    const logo = await loadImage(elements.brandLogo.src);
    whiteLogo = recolorImageToWhite(logo);
  } catch (_error) {}
  const scaleX = exportSize.width / CARD_WIDTH;
  const scaleY = exportSize.height / CARD_HEIGHT;
  const scaleBox = (box) => ({
    x: box.x * scaleX,
    y: box.y * scaleY,
    width: box.width * scaleX,
    height: box.height * scaleY,
    right: box.right * scaleX,
    bottom: box.bottom * scaleY
  });
  const scaledLayout = {
    brand: scaleBox(layout.brand),
    siteLabel: { ...layout.siteLabel, right: layout.siteLabel.right * scaleX, y: layout.siteLabel.y * scaleY, size: layout.siteLabel.size * scaleY },
    token: { ...layout.token, x: layout.token.x * scaleX, y: layout.token.y * scaleY, size: layout.token.size * scaleY },
    pnl: { ...scaleBox(layout.pnl), radius: layout.pnl.radius * scaleX },
    pnlSymbol: { ...scaleBox(layout.pnlSymbol), size: layout.pnlSymbol.size * scaleY },
    pnlText: { ...layout.pnlText, x: layout.pnlText.x * scaleX, y: layout.pnlText.y * scaleY, size: layout.pnlText.size * scaleY },
    metrics: layout.metrics.map((row) => ({
      ...row,
      labelX: row.labelX * scaleX,
      valueRight: row.valueRight * scaleX,
      y: row.y * scaleY,
      size: row.size * scaleY,
      iconSize: row.iconSize * scaleY,
      iconGap: row.iconGap * scaleX
    })),
    promoOne: { ...layout.promoOne, x: layout.promoOne.x * scaleX, y: layout.promoOne.y * scaleY, size: layout.promoOne.size * scaleY },
    promoTwo: { ...layout.promoTwo, x: layout.promoTwo.x * scaleX, y: layout.promoTwo.y * scaleY, size: layout.promoTwo.size * scaleY },
    handle: { ...layout.handle, x: layout.handle.x * scaleX, y: layout.handle.y * scaleY, size: layout.handle.size * scaleY }
  };
  const pnl = currentPnlValue();
  const pnlColor = pnl < 0 ? state.settings.negativePnlColor : state.settings.positivePnlColor;
  const pnlText = formatSignedMoneyText(pnl, currentPnlValueKind());
  const isUsdMode = state.settings.displayCurrency === "USD";
  const mainColor = state.settings.mainTextColor;
  const softTextColor = applyAlpha(mainColor, 0.85);
  const mutedTextColor = applyAlpha(mainColor, 0.7);
  const symbolColor = state.settings.rectangle ? state.settings.rectangleTextColor : pnlColor;
  const solIcon = isUsdMode ? null : await loadImage(solIconSvg(symbolColor));
  const metricSolIcon = state.settings.displayCurrency === "SOL"
    ? await loadImage(solIconSvg(mainColor))
    : null;
  return () => {
    ctx.clearRect(0, 0, exportSize.width, exportSize.height);
    const gradient = ctx.createLinearGradient(0, 0, exportSize.width, 0);
    gradient.addColorStop(0, "rgba(0,0,0,0.58)");
    gradient.addColorStop(0.42, "rgba(0,0,0,0.18)");
    gradient.addColorStop(1, "rgba(0,0,0,0.04)");
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, exportSize.width, exportSize.height);
    if (whiteLogo) {
      drawMediaContain(ctx, whiteLogo, scaledLayout.brand.x, scaledLayout.brand.y, scaledLayout.brand.width, scaledLayout.brand.height);
    }
    drawText("Trench.Tools", scaledLayout.siteLabel.right, scaledLayout.siteLabel.y, scaledLayout.siteLabel.size, {
      weight: scaledLayout.siteLabel.weight,
      align: "right",
      color: mainColor,
      tracking: scaledLayout.siteLabel.tracking * scaleX
    });
    drawText(tokenName(), scaledLayout.token.x, scaledLayout.token.y, scaledLayout.token.size, {
      weight: scaledLayout.token.weight,
      tracking: scaledLayout.token.tracking * scaleX,
      color: mainColor
    });
    if (state.settings.rectangle) {
      ctx.fillStyle = pnlColor;
      drawRoundedRect(ctx, scaledLayout.pnl.x, scaledLayout.pnl.y, scaledLayout.pnl.width, scaledLayout.pnl.height, scaledLayout.pnl.radius);
      ctx.fill();
    }
    if (!isUsdMode && solIcon) {
      drawMediaContain(ctx, solIcon, scaledLayout.pnlSymbol.x, scaledLayout.pnlSymbol.y, scaledLayout.pnlSymbol.width, scaledLayout.pnlSymbol.height);
    }
    drawText(pnlText, scaledLayout.pnlText.x, scaledLayout.pnlText.y, scaledLayout.pnlText.size, {
      weight: scaledLayout.pnlText.weight,
      color: state.settings.rectangle ? state.settings.rectangleTextColor : pnlColor,
      tracking: scaledLayout.pnlText.tracking * scaleX,
      shadow: state.settings.rectangle ? false : undefined
    });
    const drawMetricValue = (value, row, color, kind = "SOL") => {
      const finalColor = color || mainColor;
      if (state.settings.displayCurrency === "SOL" && kind !== "USD") {
        const converted = convertValue(value, kind);
        if (converted == null) {
          drawText("--", row.valueRight, row.y, row.size, { weight: row.valueWeight, align: "right", color: finalColor });
          return;
        }
        const numberText = formatSolValueText(converted);
        ctx.font = `${row.valueWeight} ${row.size}px ${fontStack}`;
        const textWidth = ctx.measureText(numberText).width;
        const iconX = row.valueRight - textWidth - row.iconGap - row.iconSize;
        const iconY = row.y + (row.size - row.iconSize) / 2;
        if (metricSolIcon) {
          drawMediaContain(ctx, metricSolIcon, iconX, iconY, row.iconSize, row.iconSize);
        }
        drawText(numberText, row.valueRight, row.y, row.size, { weight: row.valueWeight, align: "right", color: finalColor });
      } else {
        drawText(formatMoney(value, kind), row.valueRight, row.y, row.size, { weight: row.valueWeight, align: "right", color: finalColor });
      }
    };
    const [pnlRow, investedRow, positionRow] = scaledLayout.metrics;
    drawText("PNL", pnlRow.labelX, pnlRow.y, pnlRow.size, { weight: pnlRow.labelWeight, color: mutedTextColor });
    drawText(formatPercent(currentPnlPercent()), pnlRow.valueRight, pnlRow.y, pnlRow.size, {
      weight: pnlRow.valueWeight,
      align: "right",
      color: pnlColor
    });
    drawText("Invested", investedRow.labelX, investedRow.y, investedRow.size, { weight: investedRow.labelWeight, color: mutedTextColor });
    drawMetricValue(currentInvestedValue(), investedRow, undefined, currentInvestedValueKind());
    drawText("Position", positionRow.labelX, positionRow.y, positionRow.size, { weight: positionRow.labelWeight, color: mutedTextColor });
    drawMetricValue(currentPositionValue(), positionRow, undefined, currentPositionValueKind());
    drawText(PROMO_LINE_ONE, scaledLayout.promoOne.x, scaledLayout.promoOne.y, scaledLayout.promoOne.size, {
      weight: scaledLayout.promoOne.weight,
      color: softTextColor,
      tracking: scaledLayout.promoOne.tracking * scaleX
    });
    drawText(PROMO_LINE_TWO, scaledLayout.promoTwo.x, scaledLayout.promoTwo.y, scaledLayout.promoTwo.size, {
      weight: scaledLayout.promoTwo.weight,
      color: mainColor,
      tracking: scaledLayout.promoTwo.tracking * scaleX
    });
    drawText(state.settings.handle || "@", scaledLayout.handle.x, scaledLayout.handle.y, scaledLayout.handle.size, {
      weight: scaledLayout.handle.weight,
      tracking: scaledLayout.handle.tracking * scaleX,
      color: mainColor
    });
  };
}

async function ensureFontsReady() {
  if (!document.fonts || typeof document.fonts.load !== "function") return;
  try {
    await Promise.all([
      document.fonts.load("900 60px Inter"),
      document.fonts.load("italic 900 58px Inter"),
      document.fonts.load("800 42px Inter"),
      document.fonts.load("500 24px Inter"),
      document.fonts.load("700 24px Inter")
    ]);
    if (document.fonts.ready) {
      await document.fonts.ready;
    }
  } catch (_error) {}
}

function exportLayout() {
  const previewElement = elements.previewCard || elements.preview;
  const previewRect = previewElement.getBoundingClientRect();
  const scaleX = CARD_WIDTH / Math.max(1, previewRect.width);
  const scaleY = CARD_HEIGHT / Math.max(1, previewRect.height);
  const box = (element) => {
    if (!(element instanceof Element)) {
      throw new Error("Unable to read card preview layout.");
    }
    const rect = element.getBoundingClientRect();
    return {
      x: (rect.left - previewRect.left) * scaleX,
      y: (rect.top - previewRect.top) * scaleY,
      width: rect.width * scaleX,
      height: rect.height * scaleY,
      right: (rect.right - previewRect.left) * scaleX,
      bottom: (rect.bottom - previewRect.top) * scaleY
    };
  };
  const fontSize = (element, fallback) => {
    const value = parseFloat(window.getComputedStyle(element).fontSize);
    return Number.isFinite(value) ? value * scaleY : fallback;
  };
  const fontWeight = (element, fallback) => {
    const value = Number(window.getComputedStyle(element).fontWeight);
    return Number.isFinite(value) ? value : fallback;
  };
  const tracking = (element, fallback = 0) => {
    const raw = window.getComputedStyle(element).letterSpacing;
    const value = raw === "normal" ? 0 : parseFloat(raw);
    return Number.isFinite(value) ? value * scaleX : fallback;
  };
  const brand = box(elements.brandLogo);
  const site = box(elements.siteLabel);
  const token = box(elements.tokenName);
  const pnl = box(elements.pnlAmount);
  const pnlSymbolElement = state.settings.displayCurrency === "USD"
    ? elements.pnlCurrencySymbol
    : elements.pnlCurrencyIcon;
  const pnlSymbol = box(pnlSymbolElement);
  const pnlText = box(elements.pnlAmountText);
  const metricLabels = Array.from(previewElement.querySelectorAll(".metric-grid span"));
  const metricValues = Array.from(previewElement.querySelectorAll(".metric-grid strong"));
  const metricRows = metricLabels
    .slice(0, 3)
    .map((label, index) => {
      const value = metricValues[index];
      const labelBox = box(label);
      const valueBox = box(value);
      return {
        labelX: labelBox.x,
        valueRight: valueBox.right,
        y: labelBox.y,
        size: fontSize(label, 28),
        labelWeight: fontWeight(label, 600),
        valueWeight: fontWeight(value, state.settings.bold ? 900 : 700),
        iconSize: 20 * scaleY,
        iconGap: 8 * scaleX
      };
    });
  const promoOne = box(elements.promoLineOne);
  const promoTwo = box(elements.promoLineTwo);
  const handle = box(elements.handle);
  return {
    brand,
    siteLabel: {
      right: site.right,
      y: site.y,
      size: fontSize(elements.siteLabel, 26),
      weight: fontWeight(elements.siteLabel, state.settings.bold ? 900 : 800),
      tracking: tracking(elements.siteLabel, -0.78)
    },
    token: {
      x: token.x,
      y: token.y,
      size: fontSize(elements.tokenName, 50),
      weight: fontWeight(elements.tokenName, state.settings.bold ? 900 : 800),
      tracking: tracking(elements.tokenName, -2)
    },
    pnl: {
      x: pnl.x,
      y: pnl.y,
      width: pnl.width,
      height: pnl.height,
      radius: 4 * scaleX
    },
    pnlSymbol: {
      x: pnlSymbol.x,
      y: pnlSymbol.y,
      width: pnlSymbol.width,
      height: pnlSymbol.height,
      size: fontSize(pnlSymbolElement, 64),
      weight: fontWeight(pnlSymbolElement, 900),
      tracking: tracking(pnlSymbolElement, -3)
    },
    pnlText: {
      x: pnlText.x,
      y: pnlText.y,
      size: fontSize(elements.pnlAmountText, 64),
      weight: fontWeight(elements.pnlAmountText, 900),
      tracking: tracking(elements.pnlAmountText, -3)
    },
    metrics: metricRows,
    promoOne: {
      x: promoOne.x,
      y: promoOne.y,
      size: fontSize(elements.promoLineOne, 23),
      weight: fontWeight(elements.promoLineOne, 600),
      tracking: tracking(elements.promoLineOne, -0.5)
    },
    promoTwo: {
      x: promoTwo.x,
      y: promoTwo.y,
      size: fontSize(elements.promoLineTwo, 30),
      weight: fontWeight(elements.promoLineTwo, 900),
      tracking: tracking(elements.promoLineTwo, -1)
    },
    handle: {
      x: handle.x,
      y: handle.y,
      size: fontSize(elements.handle, 52),
      weight: fontWeight(elements.handle, state.settings.bold ? 900 : 800),
      tracking: tracking(elements.handle, -1.2)
    }
  };
}

function drawRoundedRect(ctx, x, y, width, height, radius) {
  const r = Math.min(radius, width / 2, height / 2);
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.lineTo(x + width - r, y);
  ctx.quadraticCurveTo(x + width, y, x + width, y + r);
  ctx.lineTo(x + width, y + height - r);
  ctx.quadraticCurveTo(x + width, y + height, x + width - r, y + height);
  ctx.lineTo(x + r, y + height);
  ctx.quadraticCurveTo(x, y + height, x, y + height - r);
  ctx.lineTo(x, y + r);
  ctx.quadraticCurveTo(x, y, x + r, y);
  ctx.closePath();
}

function loadImage(url) {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("Unable to load card image."));
    image.src = url;
  });
}

function drawMediaCover(ctx, media, x, y, width, height) {
  const sourceWidth = Number(media.videoWidth || media.naturalWidth || media.width) || width;
  const sourceHeight = Number(media.videoHeight || media.naturalHeight || media.height) || height;
  const scale = Math.max(width / sourceWidth, height / sourceHeight);
  const cropWidth = width / scale;
  const cropHeight = height / scale;
  const sourceX = Math.max(0, (sourceWidth - cropWidth) / 2);
  const sourceY = Math.max(0, (sourceHeight - cropHeight) / 2);
  ctx.drawImage(media, sourceX, sourceY, cropWidth, cropHeight, x, y, width, height);
}

function drawVideoSectionCover(ctx, media, x, y, width, height) {
  const sourceWidth = Number(media.videoWidth || media.naturalWidth || media.width) || width;
  const sourceHeight = Number(media.videoHeight || media.naturalHeight || media.height) || height;
  const targetAspect = width / height;
  const sourceAspect = sourceWidth / sourceHeight;
  if (!Number.isFinite(sourceAspect) || sourceAspect <= 0) {
    drawMediaCover(ctx, media, x, y, width, height);
    return;
  }
  let cropWidth = sourceWidth;
  let cropHeight = sourceHeight;
  let sourceX = 0;
  let sourceY = 0;
  if (sourceAspect > targetAspect) {
    cropWidth = sourceHeight * targetAspect;
    sourceX = (sourceWidth - cropWidth) / 2;
  } else {
    cropHeight = sourceWidth / targetAspect;
    const rangeY = Math.max(0, sourceHeight - cropHeight) / 2;
    sourceY = ((sourceHeight - cropHeight) / 2) + rangeY * (videoSectionOffset() / 100);
  }
  ctx.drawImage(media, sourceX, sourceY, cropWidth, cropHeight, x, y, width, height);
}

function drawMediaContain(ctx, media, x, y, width, height) {
  const sourceWidth = Number(media.videoWidth || media.naturalWidth || media.width) || width;
  const sourceHeight = Number(media.videoHeight || media.naturalHeight || media.height) || height;
  const scale = Math.min(width / sourceWidth, height / sourceHeight);
  const drawWidth = sourceWidth * scale;
  const drawHeight = sourceHeight * scale;
  ctx.drawImage(media, x + (width - drawWidth) / 2, y + (height - drawHeight) / 2, drawWidth, drawHeight);
}

function setPreviewMedia(media) {
  const isVideo = media.kind === "video";
  elements.mediaImage.classList.toggle("hidden", isVideo);
  elements.mediaVideo.classList.toggle("hidden", !isVideo);
  elements.previewBackgroundScrim.classList.toggle("hidden", !isVideo);
  if (isVideo) {
    if (!media.url) {
      elements.mediaImage.classList.remove("hidden");
      elements.previewBackgroundImage.classList.remove("hidden");
      elements.previewBackgroundVideo.classList.add("hidden");
      const fallbackUrl = actualMediaUrl(
        media.thumbnailUrl || state.previewImageUrl || BUILT_IN_TEMPLATES[0].url
      );
      swapPreviewImage(fallbackUrl);
      elements.previewBackgroundImage.src = fallbackUrl;
      if (state.previewVideoUrl) {
        elements.mediaVideo.pause();
        elements.mediaVideo.removeAttribute("src");
        elements.mediaVideo.load();
        state.previewVideoUrl = "";
      }
      if (state.previewBackgroundVideoUrl) {
        elements.previewBackgroundVideo.pause();
        elements.previewBackgroundVideo.removeAttribute("src");
        elements.previewBackgroundVideo.load();
        state.previewBackgroundVideoUrl = "";
      }
      updatePreviewVideoAspectFromMedia(media);
      return;
    }
    const videoUrl = actualMediaUrl(media.url);
    if (state.previewVideoUrl !== videoUrl) {
      elements.mediaVideo.src = videoUrl;
      state.previewVideoUrl = videoUrl;
      state.pendingVideoMetadataUrl = "";
    }
    elements.previewBackgroundImage.classList.add("hidden");
    elements.previewBackgroundVideo.classList.remove("hidden");
    if (state.previewBackgroundVideoUrl !== videoUrl) {
      elements.previewBackgroundVideo.src = videoUrl;
      state.previewBackgroundVideoUrl = videoUrl;
    }
    if (updatePreviewVideoAspectFromVideo(elements.mediaVideo)) {
      state.pendingVideoMetadataUrl = "";
      syncVideoSectionOffsetStyle();
    } else if (state.pendingVideoMetadataUrl !== videoUrl) {
      state.pendingVideoMetadataUrl = videoUrl;
      elements.mediaVideo.addEventListener("loadedmetadata", () => {
        if (state.previewVideoUrl !== videoUrl) return;
        state.pendingVideoMetadataUrl = "";
        updatePreviewVideoAspectFromVideo(elements.mediaVideo);
        syncVideoSectionOffsetStyle();
      }, { once: true });
    }
    tryPlayPreviewVideo();
  } else {
    elements.previewBackgroundImage.classList.add("hidden");
    elements.previewBackgroundVideo.classList.add("hidden");
    const imageUrl = actualMediaUrl(
      media.url || media.thumbnailUrl || state.previewImageUrl || BUILT_IN_TEMPLATES[0].url
    );
    swapPreviewImage(imageUrl);
    if (state.previewVideoUrl) {
      elements.mediaVideo.pause();
      elements.mediaVideo.removeAttribute("src");
      elements.mediaVideo.load();
      state.previewVideoUrl = "";
      state.pendingVideoMetadataUrl = "";
    }
    if (state.previewBackgroundVideoUrl) {
      elements.previewBackgroundVideo.pause();
      elements.previewBackgroundVideo.removeAttribute("src");
      elements.previewBackgroundVideo.load();
      state.previewBackgroundVideoUrl = "";
    }
  }
}

function updatePreviewVideoAspectFromMedia(media) {
  const width = Number(media?.width || media?.videoWidth) || 0;
  const height = Number(media?.height || media?.videoHeight) || 0;
  if (width > 0 && height > 0) {
    state.previewVideoAspect = width / height;
    return true;
  }
  return false;
}

function updatePreviewVideoAspectFromVideo(video) {
  const width = Number(video?.videoWidth) || 0;
  const height = Number(video?.videoHeight) || 0;
  if (width > 0 && height > 0) {
    state.previewVideoAspect = width / height;
    return true;
  }
  return false;
}

function swapPreviewImage(url) {
  if (!url) return;
  if (state.previewImageUrl === url) return;
  if (state.pendingPreviewImage === url) return;
  state.pendingPreviewImage = url;
  const next = new Image();
  next.decoding = "async";
  next.src = url;
  const apply = () => {
    if (state.pendingPreviewImage !== url) return;
    state.pendingPreviewImage = "";
    state.previewImageUrl = url;
    elements.mediaImage.src = url;
  };
  const decodePromise = typeof next.decode === "function" ? next.decode() : null;
  if (decodePromise && typeof decodePromise.then === "function") {
    decodePromise.then(apply).catch(() => {
      if (state.pendingPreviewImage !== url) return;
      state.pendingPreviewImage = "";
      state.previewImageUrl = url;
      elements.mediaImage.src = url;
    });
  } else {
    next.onload = apply;
    next.onerror = () => {
      if (state.pendingPreviewImage !== url) return;
      state.pendingPreviewImage = "";
      state.previewImageUrl = url;
      elements.mediaImage.src = url;
    };
  }
}

async function exportMediaElement() {
  if (selectedMedia().kind !== "video") {
    if (selectedMedia().uploaded && !selectedMedia().url) {
      throw new Error("Still loading uploaded media.");
    }
    return await loadImage(elements.mediaImage.src);
  }
  if (!selectedMedia().url) {
    throw new Error("Still loading uploaded video.");
  }
  await ensureVideoFrame(elements.mediaVideo);
  return elements.mediaVideo;
}

function requestMediaData(media) {
  if (!media?.uploaded || !media.id || media.url || state.pendingMediaData.has(media.id)) {
    return;
  }
  state.pendingMediaData.add(media.id);
  emit("pnl-card-get-media-data", { mediaId: media.id });
}

function waitForMediaData(mediaId, timeoutMs = 20000) {
  const id = String(mediaId || "").trim();
  if (!id) {
    return Promise.resolve("");
  }
  const existing = state.mediaDataUrls.get(id);
  if (existing) {
    return Promise.resolve(existing);
  }
  return new Promise((resolve, reject) => {
    const entry = {
      resolve: (dataUrl) => {
        window.clearTimeout(timeout);
        resolve(dataUrl);
      },
      reject: (error) => {
        window.clearTimeout(timeout);
        reject(error);
      }
    };
    const timeout = window.setTimeout(() => {
      const waiters = state.mediaDataWaiters.get(id) || [];
      const remaining = waiters.filter((waiter) => waiter !== entry);
      if (remaining.length) {
        state.mediaDataWaiters.set(id, remaining);
      } else {
        state.mediaDataWaiters.delete(id);
        state.pendingMediaData.delete(id);
      }
      entry.reject(new Error("Still loading uploaded video."));
    }, timeoutMs);
    const waiters = state.mediaDataWaiters.get(id) || [];
    waiters.push(entry);
    state.mediaDataWaiters.set(id, waiters);
  });
}

async function ensureSelectedMediaData() {
  const media = selectedMedia();
  if (!media?.uploaded || media.url) {
    return media;
  }
  requestMediaData(media);
  const dataUrl = await waitForMediaData(media.id);
  return { ...media, url: dataUrl };
}

function applyMediaData(payload) {
  const mediaId = String(payload?.id || "").trim();
  const dataUrl = String(payload?.dataUrl || "").trim();
  state.pendingMediaData.delete(mediaId);
  if (!mediaId || !dataUrl) {
    const waiters = state.mediaDataWaiters.get(mediaId) || [];
    state.mediaDataWaiters.delete(mediaId);
    waiters.forEach((entry) => entry.reject(new Error("Still loading uploaded video.")));
    return;
  }
  state.mediaDataUrls.set(mediaId, dataUrl);
  const waiters = state.mediaDataWaiters.get(mediaId) || [];
  state.mediaDataWaiters.delete(mediaId);
  waiters.forEach((entry) => entry.resolve(dataUrl));
  render();
}

function applyMediaSaved(payload) {
  const mediaId = String(payload?.mediaId || payload?.media?.id || "").trim();
  if (mediaId) {
    state.settings.mediaId = mediaId;
    state.settings.templateId = "";
    state.localSettingsDirty = true;
    setNotice("Uploaded media.");
    persistSettings();
  } else {
    setNotice("Uploaded media.");
  }
}

function ensureVideoFrame(video) {
  return new Promise((resolve, reject) => {
    if (video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) {
      resolve();
      return;
    }
    const timeout = window.setTimeout(() => {
      cleanup();
      reject(new Error("Unable to load card video."));
    }, 5000);
    const cleanup = () => {
      window.clearTimeout(timeout);
      video.removeEventListener("loadeddata", onLoaded);
      video.removeEventListener("error", onError);
    };
    const onLoaded = () => {
      cleanup();
      resolve();
    };
    const onError = () => {
      cleanup();
      reject(new Error("Unable to load card video."));
    };
    video.addEventListener("loadeddata", onLoaded, { once: true });
    video.addEventListener("error", onError, { once: true });
    video.load();
  });
}

function scheduleResize() {
  requestAnimationFrame(() => {
    const rect = document.body.getBoundingClientRect();
    emit("pnl-card-resize", {
      width: Math.ceil(rect.width),
      height: Math.ceil(rect.height)
    });
  });
}

function setNotice(message) {
  elements.notice.textContent = message || "";
}
