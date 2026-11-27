// Copy buttons: anything with data-copy puts that text on the clipboard.
document.addEventListener("click", function (e) {
  var b = e.target.closest("[data-copy]");
  if (!b || !navigator.clipboard) return;
  e.preventDefault();
  navigator.clipboard.writeText(b.getAttribute("data-copy")).then(function () {
    var was = b.textContent;
    b.textContent = "Copied";
    setTimeout(function () { b.textContent = was; }, 1200);
  });
});

// The profile page's preview follows the choices as they're made.
document.addEventListener("change", function (e) {
  var form = e.target.closest("form.profile");
  var img = document.getElementById("face-preview");
  if (!form || !img) return;
  var v = function (n) { var r = form.querySelector('input[name="' + n + '"]:checked'); return r ? r.value : ""; };
  var look = "h=" + v("head") + ";e=" + v("eyes") + ";t=" + v("top") + ";c=" + v("top_colour");
  img.src = img.src.split("?")[0] + "?look=" + encodeURIComponent(look);
});
