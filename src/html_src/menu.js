window.RSCApp = window.RSCApp || {};

(function (app) {
  const RSC = window.RSC_CONSTANTS;

  function rotate(selector, start, angle, duration) {
    selector.each(function () {
      const element = this;
      if (element._rotationAnimation) {
        element._rotationAnimation.cancel();
      }

      const animation = element.animate(
        [
          { transform: `rotate(${start}deg)` },
          { transform: `rotate(${angle}deg)` },
        ],
        {
          duration,
          easing: "cubic-bezier(0.65, 0, 0.35, 1)",
        },
      );

      element._rotationAnimation = animation;
      animation.onfinish = function () {
        element._rotationAnimation = null;
        element.style.transform = `rotate(${angle}deg)`;
      };
      animation.oncancel = function () {
        element._rotationAnimation = null;
      };
    });
  }

  function animateDropdownPanel(slider, opening, onComplete) {
    const element = slider[0];
    if (!element) return;

    if (element._dropdownAnimation) {
      element._dropdownAnimation.cancel();
    }

    element.style.display = "block";
    element.style.overflow = "hidden";

    const currentHeight = element.getBoundingClientRect().height;
    const startHeight = opening ? 0 : currentHeight;
    const endHeight = opening ? element.scrollHeight : 0;
    element.style.height = `${startHeight}px`;

    const animation = element.animate(
      [{ height: `${startHeight}px` }, { height: `${endHeight}px` }],
      {
        duration: RSC.animation.dropdownMs,
        easing: "cubic-bezier(0.65, 0, 0.35, 1)",
      },
    );

    element._dropdownAnimation = animation;
    animation.onfinish = function () {
      element._dropdownAnimation = null;
      element.style.height = "";
      element.style.overflow = "";
      if (!opening) {
        element.style.display = "none";
      }
      onComplete();
    };
    animation.oncancel = function () {
      element._dropdownAnimation = null;
    };
  }

  function closeMenu() {
    $(RSC.selectors.menu).animate({ left: "-25%" }, RSC.animation.menuMs);
    $(RSC.selectors.mainContent)
      .animate({ left: "0%" }, RSC.animation.menuMs)
      .animate(
        { "background-color": "var(--pageBG)" },
        RSC.animation.overlayMs,
      );
  }

  function fadeOverlayOut() {
    $(RSC.selectors.overlay).fadeTo(
      RSC.animation.overlayMs,
      0.0,
      "linear",
      function () {
        $(RSC.selectors.grad).hide();
      },
    );
  }

  function dropdownClick(event) {
    event.preventDefault();
    const dropdown = $(this).closest(".CentralMenuDropdown");
    const slider = dropdown.find(".dropdownDrop");
    const arrow = dropdown.find(".dropdownArrow");

    if (slider.hasClass("open")) {
      rotate(arrow.find("svg"), 0, 180, RSC.animation.arrowMs);
      animateDropdownPanel(slider, false, function () {
        slider.removeClass("open");
        dropdown.removeClass("open");
      });
    } else {
      rotate(arrow.find("svg"), 180, 0, RSC.animation.arrowMs);
      slider.addClass("open");
      dropdown.addClass("open");
      animateDropdownPanel(slider, true, function () {});
    }
  }

  function installEasing() {
    $.easing.expoEaseInOut = function (x, t, b, c, d) {
      if (t === 0) return b;
      if (t === d) return b + c;
      if ((t /= d / 2) < 1) return (c / 2) * Math.pow(2, 10 * (t - 1)) + b;
      return (c / 2) * (-Math.pow(2, -10 * --t) + 2) + b;
    };
  }

  app.initNavigation = function () {
    installEasing();
    $(RSC.selectors.menu).animate({ left: "-25%" }, 1);

    $(".menuBTN1").click(function (e) {
      if (e.which !== 1) return;
      app.sendSocketMessage({ type: RSC.messages.terminateServers });
    });

    const classMap = {
      Servers: ".servers",
      Configuration: ".config",
      Stats: ".stat",
    };
    $("#menu ul li a").click(function (e) {
      const targetSelector = classMap[e.target.innerHTML];
      if (!targetSelector) return;
      $(".active").toggleClass("active");
      $(e.target.parentElement).toggleClass("active");
      $(".page").hide();
      $(RSC.selectors.grad).show();
      $(targetSelector).show();
    });

    $("#menu ul li").first().toggleClass("active");
    $(".page").hide();
    $(".servers").show();

    let menuOpen = false;
    $("#menu-icon").click(function () {
      if (menuOpen) {
        closeMenu();
        fadeOverlayOut();
      } else {
        $(RSC.selectors.menu).animate({ left: "0%" }, RSC.animation.menuMs);
        $(RSC.selectors.mainContent).animate(
          { left: "25%" },
          RSC.animation.menuMs,
        );
        setTimeout(function () {
          $(RSC.selectors.grad).show();
          $(RSC.selectors.overlay).fadeTo(
            RSC.animation.overlayMs,
            1.0,
            "linear",
          );
        }, RSC.animation.retryMs);
      }
      setTimeout(function () {
        menuOpen = !menuOpen;
      }, RSC.animation.menuMs);
    });

    $(RSC.selectors.mainContent).hover(function () {
      if (menuOpen) {
        closeMenu();
        fadeOverlayOut();
      }
      menuOpen = false;
    });
  };

  app.dropdownClick = dropdownClick;
})(window.RSCApp);
