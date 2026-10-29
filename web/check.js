// Runs before the app: a browser without what the page needs gets told so, and which browsers work, instead of a blank
// page (REQ-019-01). The app's start script checks `goodGalleryUnsupported` and doesn't load then.
(function () {
  var missing = [];
  if (typeof WebAssembly !== "object") missing.push("WebAssembly");
  if (!(navigator.storage && navigator.storage.getDirectory)) missing.push("storage for the index");
  var canvas = document.createElement("canvas");
  if (!navigator.gpu && !canvas.getContext("webgl2")) missing.push("WebGPU or WebGL 2");
  if (!window.isSecureContext) missing.push("a secure (HTTPS) connection");
  if (missing.length === 0) return;
  window.goodGalleryUnsupported = true;
  document.addEventListener("DOMContentLoaded", function () {
    var problem = document.getElementById("problem");
    problem.textContent =
      "Good Gallery can't run in this browser: it lacks " +
      missing.join(", ") +
      ". It works in current versions of Chrome, Edge, Firefox and Safari on a computer.";
    problem.style.display = "flex";
  });
})();
