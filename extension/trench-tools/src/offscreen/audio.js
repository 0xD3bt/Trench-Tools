const activeAudio = new Set();
const ACTIVE_AUDIO_CAP = 8;

function cullActiveAudio() {
  while (activeAudio.size > ACTIVE_AUDIO_CAP) {
    const oldest = activeAudio.values().next().value;
    if (!oldest) break;
    try {
      oldest.pause();
    } catch {}
    activeAudio.delete(oldest);
  }
}

function playSound({ url, volume }) {
  if (!url) {
    return { ok: false, error: "missing url" };
  }
  try {
    const audio = new Audio(url);
    const vol = Number(volume);
    audio.volume = Number.isFinite(vol) ? Math.min(1, Math.max(0, vol)) : 0.7;
    audio.addEventListener("ended", () => {
      activeAudio.delete(audio);
    });
    audio.addEventListener("error", () => {
      activeAudio.delete(audio);
    });
    activeAudio.add(audio);
    cullActiveAudio();
    const promise = audio.play();
    if (promise && typeof promise.catch === "function") {
      promise.catch((error) => {
        activeAudio.delete(audio);
        console.warn("Trench Tools offscreen audio play failed", error);
      });
    }
    return { ok: true };
  } catch (error) {
    return { ok: false, error: error?.message || "play failed" };
  }
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type !== "trench:offscreen-play-sound") {
    return false;
  }
  const result = playSound(message.payload || {});
  sendResponse(result);
  return false;
});
