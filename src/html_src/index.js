function closeMenu() {
	$('#menu').animate({
		"left": "-25%"
	}, 250);
	$("#main-content").animate({
		"left": "0%"
	}, 250).animate({
		"background-color": "var(--pageBG)"
	}, 350);
}

function AnimateRotate(selector, start, angle, duration) {
	// caching the object for performance reasons
	var $elem = selector;
	// we use a pseudo object for the animation
	// (starts from `0` to `angle`), you can name it as you want
	$({
		deg: start
	}).animate({
		deg: angle
	}, {
		duration: duration,
		step: function(now) {
			// in the step-callback (that is fired each step of the animation),
			// you can use the `now` paramter which contains the current
			// animation-position (`0` up to `angle`)
			$elem.css({
				transform: 'rotate(' + now + 'deg)'
			});
		}
	});
}

function dropdownClick(event) {
	event.preventDefault();
	var dropdown = $(this).closest(".CentralMenuDropdown");
    dropdown.toggleClass("open");
	var slider = dropdown.find(".dropdownDrop");
	var arrow = dropdown.find(".dropdownArrow");
	// Toggle the visibility of the ".dropdownDrop" content with a slide animation
	if (slider.hasClass("open")) {
		//close, then hide
		AnimateRotate(arrow.find("svg"), 0, 180, 100);
		slider.slideUp(250, function() {
			slider.hide();
            //$(".CentralMenuDropdown").show();
		});
	} else {
		//show, then open
        //$(".CentralMenuDropdown").hide();
        //dropdown.show();
		AnimateRotate(arrow.find("svg"), 180, 0, 100);
		slider.slideDown(250, function() {
			slider.show()
		})
	}
	slider.toggleClass("open")
}

function get_ws_addr() {
	return document.location.href.replace("http", "ws").replace("https", "wss").replace("#", "") + "ws"
}

function hotReloadWhenReady() {
	setInterval(function() {
		try {
			var req = new XMLHttpRequest();
			req.onreadystatechange = function() {
				if (this.status === 200) {
					document.location.reload();
				}
			}
			req.open("GET", document.location.href);
			req.send()
		} catch (e) {}
	}, 1000);
}

function addServerDropdown(serverName, inactive) {
    let titleText = serverName;
    if (inactive) {
        titleText += " (inactive)"
    }
    var dropdown = $("<div class=\"CentralMenuDropdown "+ serverName +"dropdown\"><div class=\"innerTopBarDropDown\"> <p class=\"serverName\">"+ titleText +"</p>   <a href=\"#\" class=\"button dropdownArrow\"><svg clip-rule=\"evenodd\" class=\"bloom\" fill-rule=\"evenodd\" stroke-linejoin=\"round\" stroke-miterlimit=\"2\" viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\">  <path d=\"m16.843 10.211c.108-.141.157-.3.157-.456 0-.389-.306-.755-.749-.755h-8.501c-.445 0-.75.367-.75.755 0 .157.05.316.159.457 1.203 1.554 3.252 4.199 4.258 5.498.142.184.36.29.592.29.23 0 .449-.107.591-.291zm-7.564.289h5.446l-2.718 3.522z\" fill-rule=\"nonzero\"/>  </svg><svg clip-rule=\"evenodd\" fill-rule=\"evenodd\" stroke-linejoin=\"round\" stroke-miterlimit=\"2\" viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"m16.843 10.211c.108-.141.157-.3.157-.456 0-.389-.306-.755-.749-.755h-8.501c-.445 0-.75.367-.75.755 0 .157.05.316.159.457 1.203 1.554 3.252 4.199 4.258 5.498.142.184.36.29.592.29.23 0 .449-.107.591-.291zm-7.564.289h5.446l-2.718 3.522z\" fill-rule=\"nonzero\"/></svg></a></div><div class=\"dropdownDrop\" style: \"display: none;\"><div class=\"serverSTDOut "+ serverName +"Out\"></div><div class=\"serverSTDIn\"><input class=\"STDInInput\" placeholder=\"place input for STDIn here...\"></input><a href=\"#\" class=\"STDInSubmit\">Submit</a></div></div></div>")
	var arrow = dropdown.find(".dropdownArrow").children().css({
		transform: "rotate(-180deg)"
	});
	dropdown.appendTo(".centerMenu.servers");
    dropdown.find(".innerTopBarDropDown").click(dropdownClick);
    var commands = [];
    var numBack = 0;
    var input = dropdown.find(".STDInInput");
    input.keydown(function(e) {
        if (e.which === 13) {
            //enter pressed
            var input2 = $(this).val();
            if (input2 == "") return;
            $(this).val("");
            var obj = {
                type: "stdinInput",
                server_name: serverName,
                value: input2
            };

			var dropdow = dropdown.find("."+serverName +"dropdown").prevObject;
			console.log(dropdow);
			if (input2 == "start" && dropdow.hasClass("inactiveServer")) {
				$("." + serverName + "Out").children().remove()
			}
            if (commands.length > 25) {
                var commandsTemp = [];
				for (var i = Math.max(0, commands.length - 26); i < commands.length; ++i) {
					commandsTemp.push(commands[i]);
				}
				commands = commandsTemp;
            }
            commands.push(input2);
            numBack = 0;
            socket.send(JSON.stringify(obj));
        } else if (e.which === 40) {
            //down arrow
            numBack = Math.max(0, numBack - 1);
            $(this).val(commands[commands.length - numBack]);
        } else if (e.which === 38) {
            //up arrow
            numBack = Math.min(commands.length, numBack + 1);
            $(this).val(commands[commands.length - numBack]);
        }
    });
    dropdown.find(".STDInSubmit").click(function(e) {
        if (e.which === 1) {
            var input2 = $(input).val();
            if (input2 == "") return;
            $(input).val("");
            var obj = {
                type: "stdinInput",
                server_name: serverName,
                value: input2
            };
			commands = commandsTemp;
            commands.push(input2);
			var commandsTemp = [];
			for (var i = Math.max(0, commands.length - 26); i < commands.length; ++i) {
				commandsTemp.push(commands[i]);
			}
            numBack = 0;
            socket.send(JSON.stringify(obj));
        }
    })
	dropdown.find(".dropdownDrop").slideUp(1).hide();
    if (inactive == true) {
        dropdown.toggleClass("inactiveServer");
    }
}

function addDropdownNoDupe(name, inactive) {
	var q = $("." + name + "dropdown").toArray().length != 0
	if (q != true) {
		addServerDropdown(name, inactive);
	}
}

function createEvent(type, arguments) {
	var obj = {
		"type": type ?? [],
		"arguments": arguments ?? []
	}
	return JSON.stringify(obj);
}
$(document).ready(function() {
	var socket;
	socket = new WebSocket(get_ws_addr())
	socket.onerror = function() {
		hotReloadWhenReady()
	}
	socket.onclose = function() {
		hotReloadWhenReady()
	}
	socket.addEventListener("open", function() {
		socket.send(createEvent("requestInfo", [true]));
	})
    setInterval(function() {
		try {
        socket.send(createEvent("requestInfo", [false]));
        for (var index in window.serverInfoObj.servers) {
            	var server = window.serverInfoObj.servers[index];
            	var name = server.name;
            	var dropdown = $("." + name + "dropdown");
            	var title = dropdown.find(".serverName");
            	var titleText = title[0].textContent;
            	if (server.active == false && titleText.endsWith(" (inactive)") == false) {
                	title[0].textContent += " (inactive)"
            	    dropdown.toggleClass("inactiveServer");
            	}
            	if (dropdown.hasClass("inactiveServer") && server.active) {
                	title[0].textContent = title[0].textContent.split(" (inactive)").join("");
                	dropdown.toggleClass("inactiveServer");
            	}
        	}
		} catch(e) {}
    }, 200);
	window.config = {
		state: "NotInit"
	}
	var justStarted = true;
	socket.onmessage = function(message) {
		var obj = JSON.parse(message.data);
		//console.log(obj);
		switch (obj.type) {
			case "ServerInfo":
				for (index in obj.servers) {
                    var server = obj.servers[index];
					let serverName = server.name;
					addDropdownNoDupe(serverName, !server.active)
					if (justStarted) {
						window.config = obj.config;
						$(".editorText").val(JSON.stringify(obj.config, undefined, 4))
						justStarted = false;
						var lines = server.output.split("\r\n");
						for (linePos in lines) {
							var line = lines[linePos];
							var p = $("<p class=\"STDOutMessage\"></p>").appendTo("." + serverName + "Out")[0]
							p.textContent = line;
						}
					}
					if (JSON.stringify(window.config) != JSON.stringify(obj.config)) {
						//the config is out of sync, copy it to the config textarea
						$(".editorText").val(JSON.stringify(obj.config, undefined, 4))
						window.config = obj.config;
					}
				}
                window.serverInfoObj = obj;
			break;
			case "ServerOutput":
				var str = obj.output;
				var lines = str.split("\r\n");
				for (var i in lines) {
					var line = lines[i];
					if (line != "") {
                        var outDiv = $("." + obj.server_name + "Out")[0]
                        var shouldScroll = outDiv.scrollTop == outDiv.scrollHeight
						var p = $(" <p class=\"STDOutMessage\"></p>").appendTo(outDiv)[0];
						p.textContent = line;
                        if (true) {
                            outDiv.scrollTop = outDiv.scrollHeight
                        }
					}
				}
			break;
		}
	}
	if (typeof InstallTrigger !== 'undefined') { // Check if Firefox
		document.getElementById('scrollable-div').classList.add('scrollbar'); // Add the Firefox-specific class
	}
	var saveTimeout = undefined;
	$(".editorText").keypress(function(e) {
		var currentTextareaVal = $(this).val();
		var newConfig = JSON.parse(currentTextareaVal)
		if (JSON.stringify(window.config) !== JSON.stringify(newConfig)) {
			//the value of the config has changed, update the server
			if (saveTimeout != undefined) {
				clearTimeout(saveTimeout);
			}
			var obj = {
				type: "configChange",
				updatedConfig: newConfig
			}
			saveTimeout = setTimeout(function() {
				socket.send(JSON.stringify(obj));
				alert("Config has been saved, all servers restarting...")
			}, 5000);
		}
	})
	window.socket = socket
	$('#menu').animate({
		"left": "-25%"
	}, 1);
    $(".menuBTN1").click(function(e) {
        if (e.which != 1) return;
        console.log("Terminating all servers...")
        socket.send(createEvent("terminateServers"));
    })
	var classMap = {
		"Servers": ".servers",
		"Configuration": ".config",
		"Stats": ".stat"
	}
	$("#menu ul li a").click(function(e) {
		$(".active").toggleClass("active");
		$(e.target.parentElement).toggleClass("active");
		$(".page").hide();
		$(".grad").show();
		$(classMap[e.target.innerHTML]).show();
	})
	$($("#menu ul li")[0]).toggleClass("active");
	$(".page").hide();
	$(".servers").show();
	var menuOpen = false;
	$('#menu-icon').click(function() {
		if (menuOpen) {
			closeMenu();
			$(".overlay").fadeTo(350, 0.0, "linear", function() {
				$(".grad").hide();
			});
		} else {
			$('#menu').animate({
				"left": "0%"
			}, 250);
			$("#main-content").animate({
				"left": "25%",
			}, 250)
			setTimeout(function() {
				$(".grad").show();
				$(".overlay").fadeTo(350, 1.0, "linear");
			}, 100)
		}
		setTimeout(function() {
			menuOpen = !menuOpen;
		}, 250);
	});
	$("#main-content").hover(function() {
		if (menuOpen) {
			closeMenu();
			$(".overlay").fadeTo(350, 0.0, "linear", function() {
				$(".grad").hide();
			});
		}
		menuOpen = false;
	});
	var canvas = $("<canvas class=\"overlay\" width=" + 800 + " height=" + 2 + "></canvas>").appendTo($(".grad"))[0]
	var ctx = canvas.getContext("2d");
	var dropdownSelector = $(".centerMenu.CentralMenuDropdown")
	console.log(dropdownSelector);
	dropdownSelector.click();

	function handleCanvas() {
		if (window.innerWidth != canvas.width || (window.innerHeight != canvas.height)) {
			$(canvas).remove();
			canvas = $(" <canvas class=\"overlay\" width=" + window.innerWidth + " height=" + window.innerHeight + "></canvas>").appendTo($(".grad"))[0]
			ctx = canvas.getContext("2d");
			var bg = "rgba(14, 14, 14, 0.85)"
			var highlight = "rgba(162, 0, 255, 1)"
			var grad = ctx.createLinearGradient(0, 0, canvas.width, 0);
			grad.addColorStop(0, highlight);
			grad.addColorStop(0.05, bg);
			grad.addColorStop(1, bg);
			ctx.fillStyle = grad;
			ctx.fillRect(0, 0, canvas.width, canvas.height);
		}
		requestAnimationFrame(handleCanvas);
	}
	handleCanvas();
});