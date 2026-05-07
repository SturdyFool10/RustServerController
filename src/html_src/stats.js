window.RSCApp = window.RSCApp || {};

(function (app) {
  const RSC = window.RSC_CONSTANTS;

  function serverList() {
    return Array.isArray(window.serverInfoObj?.servers)
      ? window.serverInfoObj.servers
      : [];
  }

  function aggregateStats(servers) {
    const stats = {
      total: servers.length,
      active: 0,
      inactive: 0,
      specialized: 0,
      ready: 0,
      players: 0,
      maxPlayers: 0,
      specializations: new Map(),
      activeNames: [],
      inactiveNames: [],
    };

    servers.forEach((server) => {
      if (server.active) {
        stats.active += 1;
        stats.activeNames.push(server.name);
      } else {
        stats.inactive += 1;
        stats.inactiveNames.push(server.name);
      }

      const specialization = server.specialization || "Generic";
      if (server.specialization) stats.specialized += 1;
      stats.specializations.set(
        specialization,
        (stats.specializations.get(specialization) || 0) + 1,
      );

      const info = server.specialized_info || {};
      stats.players += Number(info.player_count || 0);
      stats.maxPlayers += Number(info.max_players || 0);
      if (info.ready === true) stats.ready += 1;
    });

    return stats;
  }

  function percent(value, total) {
    if (!total) return 0;
    return Math.round((value / total) * 100);
  }

  function setText(parent, selector, value) {
    const element = parent.querySelector(selector);
    if (element) element.textContent = value;
  }

  function renderList(parent, selector, names, emptyText) {
    const element = parent.querySelector(selector);
    if (!element) return;
    element.replaceChildren();

    const values = names.length ? names : [emptyText];
    values.forEach((name) => {
      const item = document.createElement("li");
      item.textContent = name;
      element.appendChild(item);
    });
  }

  function renderSpecializations(root, stats) {
    const list = root.querySelector(".statsSpecializations");
    if (!list) return;
    list.replaceChildren();

    Array.from(stats.specializations.entries())
      .sort(([left], [right]) => left.localeCompare(right))
      .forEach(([name, count]) => {
        const item = document.createElement("li");
        const label = document.createElement("span");
        const value = document.createElement("strong");
        label.textContent = name;
        value.textContent = String(count);
        item.append(label, value);
        list.appendChild(item);
      });
  }

  function formatStatValue(value) {
    if (typeof value === "boolean") return value ? "yes" : "no";
    if (Array.isArray(value)) {
      return value
        .map((entry) =>
          entry && typeof entry === "object" ? JSON.stringify(entry) : String(entry),
        )
        .join(", ");
    }
    if (value && typeof value === "object") return JSON.stringify(value);
    return String(value);
  }

  function formatHours(value) {
    const hours = Number(value || 0);
    return `${hours.toFixed(2)}h`;
  }

  function formatTimestamp(value) {
    if (!value) return "never";
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return String(value);
    return date.toLocaleString();
  }

  function renderPlayerActivity(description, players) {
    const playerList = document.createElement("ul");
    playerList.className = "statsPlayerActivity";

    if (!Array.isArray(players) || players.length === 0) {
      const empty = document.createElement("li");
      empty.textContent = "No tracked players yet";
      playerList.appendChild(empty);
      description.appendChild(playerList);
      return;
    }

    players.forEach((player) => {
      const item = document.createElement("li");
      const name = document.createElement("strong");
      const details = document.createElement("span");
      name.textContent = player.name || "Unknown player";
      details.textContent = [
        player.online ? "online" : "offline",
        `${player.sessions || 0} sessions`,
        `${formatHours(player.total_hours)} total`,
        `joined ${formatTimestamp(player.last_joined_at)}`,
        `left ${formatTimestamp(player.last_left_at)}`,
      ].join(" | ");
      item.append(name, details);
      playerList.appendChild(item);
    });

    description.appendChild(playerList);
  }

  function renderRecentSessions(description, sessions) {
    const sessionList = document.createElement("ul");
    sessionList.className = "statsPlayerActivity";

    if (!Array.isArray(sessions) || sessions.length === 0) {
      const empty = document.createElement("li");
      empty.textContent = "No completed sessions yet";
      sessionList.appendChild(empty);
      description.appendChild(sessionList);
      return;
    }

    sessions.forEach((session) => {
      const item = document.createElement("li");
      const name = document.createElement("strong");
      const details = document.createElement("span");
      name.textContent = session.name || "Unknown player";
      details.textContent = [
        `joined ${formatTimestamp(session.joined_at)}`,
        session.left_at ? `left ${formatTimestamp(session.left_at)}` : "still online",
        `${formatHours(session.duration_hours)} played`,
      ].join(" | ");
      item.append(name, details);
      sessionList.appendChild(item);
    });

    description.appendChild(sessionList);
  }

  function timeframeLabel(name) {
    return name.charAt(0).toUpperCase() + name.slice(1);
  }

  function maxValue(values, selector) {
    return values.reduce((max, value) => Math.max(max, Number(selector(value) || 0)), 0);
  }

  function renderBars(values, selector, labeler) {
    const chart = document.createElement("div");
    chart.className = "statsBars";
    const max = maxValue(values, selector) || 1;

    values.forEach((value) => {
      const bar = document.createElement("span");
      const amount = Number(selector(value) || 0);
      bar.style.height = `${Math.max(4, (amount / max) * 100)}%`;
      bar.title = labeler(value, amount);
      chart.appendChild(bar);
    });

    return chart;
  }

  function renderTimeframeStats(description, stats) {
    const wrapper = document.createElement("div");
    wrapper.className = "statsTimeframes";
    const entries = Object.entries(stats || {});

    if (!entries.length) {
      wrapper.textContent = "No timeframe data yet";
      description.appendChild(wrapper);
      return;
    }

    entries.forEach(([name, value]) => {
      const card = document.createElement("section");
      const heading = document.createElement("h3");
      const metrics = document.createElement("dl");
      const playerSamples = Array.isArray(value.player_count_samples)
        ? value.player_count_samples
        : [];
      const busyByHour = Array.isArray(value.busy_by_hour) ? value.busy_by_hour : [];

      heading.textContent = timeframeLabel(name);
      metrics.innerHTML = `
        <dt>Logged Hours</dt><dd>${formatHours(value.logged_hours)}</dd>
        <dt>Unique Players</dt><dd>${value.unique_players || 0}</dd>
        <dt>Average Online</dt><dd>${Number(value.average_online || 0).toFixed(2)}</dd>
        <dt>Peak Online</dt><dd>${value.peak_online || 0}</dd>
      `;

      card.append(heading, metrics);
      card.appendChild(
        renderBars(
          playerSamples,
          (sample) => sample.players,
          (sample, amount) =>
            `${amount} online at ${formatTimestamp(sample.timestamp)}`,
        ),
      );
      card.appendChild(
        renderBars(
          busyByHour,
          (hour) => hour.average_online,
          (hour, amount) =>
            `${Number(amount).toFixed(2)} average online at ${String(hour.hour).padStart(2, "0")}:00`,
        ),
      );

      wrapper.appendChild(card);
    });

    description.appendChild(wrapper);
  }

  function renderStatDescription(description, label, value) {
    if (label === "Player Activity") {
      renderPlayerActivity(description, value);
      return;
    }
    if (label === "Recent Sessions") {
      renderRecentSessions(description, value);
      return;
    }
    if (label === "Timeframe Stats") {
      renderTimeframeStats(description, value);
      return;
    }
    description.textContent = formatStatValue(value);
  }

  function renderSpecializationStats(root, servers) {
    const list = root.querySelector(".statsSpecializationDetails");
    if (!list) return;
    list.replaceChildren();

    const statServers = servers.filter(
      (server) =>
        server.specialization_stats &&
        typeof server.specialization_stats === "object" &&
        Object.keys(server.specialization_stats).length > 0,
    );

    const values = statServers.length ? statServers : [{ name: "No specialization stats yet" }];
    values.forEach((server) => {
      const item = document.createElement("li");
      const title = document.createElement("strong");
      title.textContent = server.name;
      item.appendChild(title);

      if (server.specialization_stats) {
        const detailList = document.createElement("dl");
        Object.entries(server.specialization_stats).forEach(([label, value]) => {
          const term = document.createElement("dt");
          const description = document.createElement("dd");
          term.textContent = label;
          renderStatDescription(description, label, value);
          detailList.append(term, description);
        });
        item.appendChild(detailList);
      }

      list.appendChild(item);
    });
  }

  function renderSpecializationSettings(root, servers) {
    const list = root.querySelector(".statsSpecializationSettings");
    if (!list) return;
    list.replaceChildren();

    const settingServers = servers.filter(
      (server) =>
        server.specialization_options &&
        typeof server.specialization_options === "object" &&
        Object.keys(server.specialization_options).length > 0,
    );

    const values = settingServers.length
      ? settingServers
      : [{ name: "No specialization settings" }];
    values.forEach((server) => {
      const item = document.createElement("li");
      const title = document.createElement("strong");
      title.textContent = server.name;
      item.appendChild(title);

      if (server.specialization_options) {
        const detailList = document.createElement("dl");
        Object.entries(server.specialization_options).forEach(([label, value]) => {
          const term = document.createElement("dt");
          const description = document.createElement("dd");
          term.textContent = label;
          description.textContent = formatStatValue(value);
          detailList.append(term, description);
        });
        item.appendChild(detailList);
      }

      list.appendChild(item);
    });
  }

  function ensureStatsMarkup() {
    const root = document.querySelector(RSC.selectors.statsRoot);
    if (!root || root.dataset.initialized === "true") return root;

    root.dataset.initialized = "true";
    root.innerHTML = `
      <section class="statsSummary">
        <div class="statBlock"><span>Total</span><strong data-stat="total">0</strong></div>
        <div class="statBlock"><span>Active</span><strong data-stat="active">0</strong></div>
        <div class="statBlock"><span>Inactive</span><strong data-stat="inactive">0</strong></div>
        <div class="statBlock"><span>Ready</span><strong data-stat="ready">0</strong></div>
      </section>
      <section class="statsBands">
        <div class="statsBand">
          <div class="statsBandHeader"><span>Server Availability</span><strong data-stat="activePercent">0%</strong></div>
          <div class="statsMeter"><span data-meter="active"></span></div>
        </div>
        <div class="statsBand">
          <div class="statsBandHeader"><span>Player Capacity</span><strong data-stat="playerCapacity">0 / 0</strong></div>
          <div class="statsMeter"><span data-meter="players"></span></div>
        </div>
      </section>
      <section class="statsColumns">
        <div>
          <h2>Specializations</h2>
          <ul class="statsSpecializations"></ul>
        </div>
        <div>
          <h2>Active Servers</h2>
          <ul class="statsActiveServers"></ul>
        </div>
        <div>
          <h2>Inactive Servers</h2>
          <ul class="statsInactiveServers"></ul>
        </div>
      </section>
      <section class="statsDetails">
        <h2>Specialization Stats</h2>
        <ul class="statsSpecializationDetails"></ul>
      </section>
      <section class="statsDetails">
        <h2>Specialization Settings</h2>
        <ul class="statsSpecializationSettings"></ul>
      </section>
    `;
    return root;
  }

  app.updateStats = function () {
    const root = ensureStatsMarkup();
    if (!root) return;

    const stats = aggregateStats(serverList());
    const servers = serverList();
    const activePercent = percent(stats.active, stats.total);
    const playerPercent = percent(stats.players, stats.maxPlayers);

    setText(root, '[data-stat="total"]', stats.total);
    setText(root, '[data-stat="active"]', stats.active);
    setText(root, '[data-stat="inactive"]', stats.inactive);
    setText(root, '[data-stat="ready"]', stats.ready);
    setText(root, '[data-stat="activePercent"]', `${activePercent}%`);
    setText(
      root,
      '[data-stat="playerCapacity"]',
      `${stats.players} / ${stats.maxPlayers}`,
    );

    const activeMeter = root.querySelector('[data-meter="active"]');
    const playerMeter = root.querySelector('[data-meter="players"]');
    if (activeMeter) activeMeter.style.width = `${activePercent}%`;
    if (playerMeter) playerMeter.style.width = `${playerPercent}%`;

    renderSpecializations(root, stats);
    renderSpecializationStats(root, servers);
    renderSpecializationSettings(root, servers);
    renderList(root, ".statsActiveServers", stats.activeNames, "No active servers");
    renderList(
      root,
      ".statsInactiveServers",
      stats.inactiveNames,
      "No inactive servers",
    );
  };

  app.initStats = function () {
    ensureStatsMarkup();
    app.updateStats();
  };
})(window.RSCApp);
