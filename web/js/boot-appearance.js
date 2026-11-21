(function () {
  try {
    var raw = localStorage.getItem("syncaidync:appearance");
    var pref = "auto";
    if (raw) {
      try {
        var v = JSON.parse(raw);
        if (v === "auto" || v === "day" || v === "night") pref = v;
      } catch (_) {}
    }
    var appearance = pref;
    if (pref === "auto") {
      var h = new Date().getHours();
      appearance = h >= 7 && h < 19 ? "day" : "night";
    }
    document.documentElement.setAttribute("data-appearance", appearance);
  } catch (_) {
    document.documentElement.setAttribute("data-appearance", "night");
  }
})();
