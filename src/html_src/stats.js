window.RSCApp = window.RSCApp || {};

(function (app) {
  const RSC = window.RSC_CONSTANTS;

  function serverList() {
    return Array.isArray(window.serverInfoObj?.servers)
      ? window.serverInfoObj.servers.filter((server) =>
          app.hasServerPermission?.(server, "stats"),
        )
      : [];
  }

  function archivedServerStats() {
    return Array.isArray(window.serverInfoObj?.archived_server_stats)
      ? window.serverInfoObj.archived_server_stats
      : [];
  }

  function globalPlayerActivity() {
    return Array.isArray(window.serverInfoObj?.global_player_activity)
      ? window.serverInfoObj.global_player_activity
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
      onlinePlayers: [],
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
      if (Array.isArray(info.player_list)) {
        info.player_list.forEach((name) => {
          if (typeof name === "string" && name.trim()) {
            stats.onlinePlayers.push({
              name: name.trim(),
              server: server.name,
              specialization,
            });
          }
        });
      }
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

  function renderSpecializationDistribution(root, stats) {
    const list = root.querySelector(".statsSpecializationDistribution");
    if (!list) return;
    list.replaceChildren();

    const entries = Array.from(stats.specializations.entries()).sort(([left], [right]) =>
      left.localeCompare(right),
    );
    if (entries.length === 0) {
      const item = document.createElement("li");
      item.textContent = "No configured servers";
      list.appendChild(item);
      return;
    }

    entries.forEach(([name, count]) => {
      const item = document.createElement("li");
      const row = document.createElement("div");
      const label = document.createElement("span");
      const value = document.createElement("strong");
      const meter = document.createElement("div");
      const fill = document.createElement("span");

      row.className = "statsDistributionHeader";
      label.textContent = name;
      value.textContent = `${count} (${percent(count, stats.total)}%)`;
      meter.className = "statsMeter";
      fill.style.width = `${percent(count, stats.total)}%`;
      meter.appendChild(fill);
      row.append(label, value);
      item.append(row, meter);
      list.appendChild(item);
    });
  }

  function renderOnlinePlayers(root, players) {
    const list = root.querySelector(".statsOnlinePlayers");
    if (!list) return;
    list.replaceChildren();

    if (!players.length) {
      const item = document.createElement("li");
      item.textContent = "No online players";
      list.appendChild(item);
      return;
    }

    players
      .slice()
      .sort((left, right) => left.server.localeCompare(right.server) || left.name.localeCompare(right.name))
      .forEach((player) => {
        const item = document.createElement("li");
        const name = document.createElement("strong");
        const details = document.createElement("span");
        name.textContent = player.name;
        details.textContent = `${player.server} | ${player.specialization}`;
        item.append(name, details);
        list.appendChild(item);
      });
  }

  function renderGlobalUserActivity(root, players) {
    const list = root.querySelector(".statsUserActivity");
    if (!list) return;
    list.replaceChildren();

    if (!players.length) {
      const item = document.createElement("li");
      item.textContent = "No observed players yet";
      list.appendChild(item);
      return;
    }

    players.forEach((player) => {
      const item = document.createElement("li");
      item.className = "statsUserActivityCard";

      const header = document.createElement("div");
      header.className = "statsUserActivityHeader";
      const name = document.createElement("strong");
      name.textContent = player.name || "Unknown player";
      const badge = document.createElement("span");
      badge.className = player.online
        ? "statsBadge statsBadge-online"
        : "statsBadge statsBadge-offline";
      badge.textContent = player.online ? "Online" : "Offline";
      const totalHours = document.createElement("span");
      totalHours.className = "statsUserActivityTotal";
      totalHours.textContent = `${formatHours(player.total_hours)} total`;
      header.append(name, badge, totalHours);

      const serverList = document.createElement("ul");
      serverList.className = "statsUserActivityServers";
      (Array.isArray(player.servers) ? player.servers : []).forEach((server) => {
        const serverItem = document.createElement("li");
        const serverBadge = document.createElement("span");
        serverBadge.className = server.online
          ? "statsBadge statsBadge-online"
          : "statsBadge statsBadge-offline";
        serverBadge.textContent = server.online ? "On now" : "Away";
        const serverName = document.createElement("span");
        serverName.className = "statsUserActivityServerName";
        serverName.textContent = server.server_name || "Unknown server";
        const serverMeta = document.createElement("span");
        serverMeta.className = "statsUserActivityServerMeta";
        serverMeta.textContent = `${formatHours(server.total_hours)} - last joined ${formatLastSeen(server.last_joined_at)}`;
        serverItem.append(serverBadge, serverName, serverMeta);
        serverList.appendChild(serverItem);
      });

      item.append(header, serverList);
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

  const ONE_YEAR_MS = 365 * 24 * 60 * 60 * 1000;

  // Like formatTimestamp, but for players we know were seen at some point
  // (they're in the remembered-users list at all) whose exact last-seen time
  // is either missing or old enough to no longer matter precisely. Rather
  // than reporting "never" - which reads as though they were dropped - this
  // keeps them visibly remembered with an approximate age.
  function formatLastSeen(value) {
    if (!value) return "more than a year ago";
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return "more than a year ago";
    if (Date.now() - date.getTime() > ONE_YEAR_MS) return "more than a year ago";
    return date.toLocaleString();
  }

  function copyText(value) {
    if (!value) return;
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(value).catch(() => {});
    }
  }

  function renderPlayerActivity(description, players) {
    const playerList = document.createElement("ul");
    playerList.className = "statsPlayerActivity";

    if (!Array.isArray(players) || players.length === 0) {
      const empty = document.createElement("li");
      empty.textContent = "No observed names yet";
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

  function renderPlayerHoursChart(description, players) {
    if (!Array.isArray(players) || players.length === 0) return;
    const values = players
      .map((player) => ({
        name: player.name || "Unknown player",
        hours: Number(player.total_hours || 0),
      }))
      .filter((player) => player.hours > 0)
      .sort((left, right) => right.hours - left.hours)
      .slice(0, 8);
    if (values.length === 0) return;

    const chart = document.createElement("div");
    chart.className = "statsPlayerHoursChart";
    const max = Math.max(...values.map((player) => player.hours), 1);

    values.forEach((player) => {
      const row = document.createElement("div");
      const label = document.createElement("span");
      const meter = document.createElement("div");
      const fill = document.createElement("span");
      const value = document.createElement("strong");

      label.textContent = player.name;
      fill.style.width = `${Math.max(2, (player.hours / max) * 100)}%`;
      value.textContent = formatHours(player.hours);
      meter.className = "statsMeter";
      meter.appendChild(fill);
      row.append(label, meter, value);
      chart.appendChild(row);
    });

    description.appendChild(chart);
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

  const WIDE_STAT_LABELS = new Set(["User Activity", "Recent Sessions", "Timeframe Stats"]);

  function timeframeLabel(name) {
    const labels = {
      day: "Rolling Day",
      week: "Rolling Week",
      month: "Rolling Month",
      year: "Rolling Year",
      all_time: "All Time",
    };
    return labels[name] || name.charAt(0).toUpperCase() + name.slice(1);
  }

  const LINE_CHART_MARGIN = 6;

  // Every ResizeObserver created by renderLineChart lands here so
  // app.updateStats can tear them all down before rebuilding the DOM on the
  // next refresh - otherwise every tick would pile up observers watching
  // canvases that have already been discarded.
  const chartResizeObservers = [];

  function disconnectChartResizeObservers() {
    chartResizeObservers.forEach((observer) => observer.disconnect());
    chartResizeObservers.length = 0;
  }

  function cssVar(name) {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  }

  // Groups `amounts` into at most `bucketCount` buckets (one per pixel
  // column) and averages each bucket down to a single value, so a series
  // with far more samples than the chart is wide on-screen never turns into
  // a canvas path with more points than there are pixels to show them on.
  // Each bucket keeps one representative original sample (its midpoint) for
  // labeling/tooltips.
  function downsampleForWidth(values, amounts, bucketCount) {
    if (amounts.length <= bucketCount) {
      return amounts.map((amount, index) => ({ amount, sample: values[index] }));
    }
    const buckets = [];
    for (let i = 0; i < bucketCount; i += 1) {
      const start = Math.floor((i * amounts.length) / bucketCount);
      const end = Math.max(start + 1, Math.floor(((i + 1) * amounts.length) / bucketCount));
      let sum = 0;
      for (let j = start; j < end; j += 1) sum += amounts[j];
      buckets.push({
        amount: sum / (end - start),
        sample: values[start + Math.floor((end - start) / 2)],
      });
    }
    return buckets;
  }

  // Renders a line graph onto a canvas using the path "shape" API
  // (beginPath/moveTo/lineTo/.../stroke) rather than a bar per sample, so it
  // can never overflow its container the way a flex row of minimum-width
  // bars could once there were more samples than available pixels. The
  // canvas is resized (backing bitmap thrown away and recreated, not
  // stretched) whenever its container's size changes, so it always exactly
  // fills the space it's given.
  function renderLineChart(values, selector, labeler, variant) {
    const chart = document.createElement("div");
    chart.className = "statsLineChart";
    const canvas = document.createElement("canvas");
    chart.appendChild(canvas);

    if (!values.length) return chart;

    const amounts = values.map((value) => Number(selector(value) || 0));
    const color = cssVar(variant === "secondary" ? "--secondary" : "--primary") || "currentColor";
    let buckets = [];

    function draw(width, height) {
      const dpr = window.devicePixelRatio || 1;
      const pixelWidth = Math.max(1, Math.round(width));
      const pixelHeight = Math.max(1, Math.round(height));

      // Recreate the backing bitmap at the new size (rather than letting the
      // old one stretch) so the line stays crisp at any container size.
      canvas.width = Math.round(pixelWidth * dpr);
      canvas.height = Math.round(pixelHeight * dpr);
      canvas.style.width = `${pixelWidth}px`;
      canvas.style.height = `${pixelHeight}px`;

      const ctx = canvas.getContext("2d");
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, pixelWidth, pixelHeight);

      const innerWidth = Math.max(1, pixelWidth - LINE_CHART_MARGIN * 2);
      const innerHeight = Math.max(1, pixelHeight - LINE_CHART_MARGIN * 2);

      const bucketCount = Math.max(1, Math.min(amounts.length, Math.round(innerWidth)));
      buckets = downsampleForWidth(values, amounts, bucketCount);
      const bucketAmounts = buckets.map((bucket) => bucket.amount);
      const max = Math.max(...bucketAmounts, 1);
      const min = Math.min(...bucketAmounts, 0);
      const range = max - min || 1;
      const stepX = buckets.length > 1 ? innerWidth / (buckets.length - 1) : 0;

      const points = buckets.map((bucket, index) => ({
        x: LINE_CHART_MARGIN + (buckets.length > 1 ? index * stepX : innerWidth / 2),
        y: LINE_CHART_MARGIN + innerHeight - ((bucket.amount - min) / range) * innerHeight,
      }));

      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.lineJoin = "round";
      ctx.lineCap = "round";
      ctx.beginPath();
      points.forEach((point, index) => {
        if (index === 0) ctx.moveTo(point.x, point.y);
        else ctx.lineTo(point.x, point.y);
      });
      ctx.stroke();

      // Marker dots every few points (plus the last one), so dense series
      // don't turn into a solid row of dots.
      const stride = Math.max(1, Math.ceil(points.length / 60));
      ctx.fillStyle = color;
      points.forEach((point, index) => {
        if (index % stride !== 0 && index !== points.length - 1) return;
        ctx.beginPath();
        ctx.arc(point.x, point.y, 2.5, 0, Math.PI * 2);
        ctx.fill();
      });
    }

    // Canvas has no per-point DOM nodes to hang a tooltip off of, so track
    // the pointer and label whichever (possibly averaged) bucket is closest.
    canvas.addEventListener("mousemove", (event) => {
      if (!buckets.length) return;
      const rect = canvas.getBoundingClientRect();
      const fraction = (event.clientX - rect.left) / rect.width;
      const index = Math.min(
        buckets.length - 1,
        Math.max(0, Math.round(fraction * (buckets.length - 1))),
      );
      const bucket = buckets[index];
      canvas.title = labeler(bucket.sample, bucket.amount);
    });

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      const box = entry.contentBoxSize?.[0];
      const width = box ? box.inlineSize : entry.contentRect.width;
      const height = box ? box.blockSize : entry.contentRect.height;
      if (width > 0 && height > 0) draw(width, height);
    });
    observer.observe(chart);
    chartResizeObservers.push(observer);

    return chart;
  }

  function renderTimeframeStats(description, stats) {
    const wrapper = document.createElement("div");
    wrapper.className = "statsTimeframes";
    // `stats` is an ordered array (day, week, month, year, all_time) from the
    // backend - do not sort or use Object.entries, which would lose that order.
    const entries = Array.isArray(stats) ? stats : [];

    if (!entries.length) {
      wrapper.textContent = "No timeframe data yet";
      description.appendChild(wrapper);
      return;
    }

    entries.forEach((value) => {
      const card = document.createElement("section");
      const heading = document.createElement("h3");
      const metrics = document.createElement("dl");
      const playerSamples = Array.isArray(value.player_count_samples)
        ? value.player_count_samples
        : [];
      const busyByHour = Array.isArray(value.busy_by_hour) ? value.busy_by_hour : [];

      heading.textContent = timeframeLabel(value.name);
      metrics.innerHTML = `
        <dt>Logged Hours</dt><dd>${formatHours(value.logged_hours)}</dd>
        <dt>Distinct Names</dt><dd>${value.distinct_names ?? 0}</dd>
        <dt>Average Online</dt><dd>${Number(value.average_online || 0).toFixed(2)}</dd>
        <dt>Peak Online</dt><dd>${value.peak_online || 0}</dd>
      `;

      card.append(heading, metrics);
      card.appendChild(
        renderLineChart(
          playerSamples,
          (sample) => sample.players,
          (sample, amount) =>
            `${amount} online at ${formatTimestamp(sample.timestamp)}`,
          "primary",
        ),
      );
      card.appendChild(
        renderLineChart(
          busyByHour,
          (hour) => hour.average_online,
          (hour, amount) =>
            `${Number(amount).toFixed(2)} average online at ${String(hour.hour).padStart(2, "0")}:00`,
          "secondary",
        ),
      );

      wrapper.appendChild(card);
    });

    description.appendChild(wrapper);
  }

  function renderStatDescription(description, label, value) {
    if (label === "User Activity") {
      renderPlayerHoursChart(description, value);
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
        appendUuidRow(detailList, server.server_uuid);
        Object.entries(server.specialization_stats).forEach(([label, value]) => {
          const term = document.createElement("dt");
          const description = document.createElement("dd");
          term.textContent = label;
          if (WIDE_STAT_LABELS.has(label)) {
            term.classList.add("statsDetailWide");
            description.classList.add("statsDetailWide");
          }
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
        appendUuidRow(detailList, server.server_uuid);
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

  function appendUuidRow(detailList, uuid) {
    if (!uuid) return;
    const term = document.createElement("dt");
    const description = document.createElement("dd");
    const uuidButton = document.createElement("button");
    term.textContent = "Server UUID";
    term.classList.add("statsUuidTerm");
    description.classList.add("statsUuidDescription");
    uuidButton.type = "button";
    uuidButton.className = "statsUuidButton";
    uuidButton.textContent = uuid;
    uuidButton.title = "Copy server UUID";
    uuidButton.addEventListener("click", () => copyText(uuid));
    description.appendChild(uuidButton);
    detailList.append(term, description);
  }

  function renderServerData(root, servers) {
    const list = root.querySelector(".statsServerData");
    if (!list) return;
    list.replaceChildren();

    const values = servers.length ? servers : [{ name: "No configured servers" }];
    values.forEach((server) => {
      const item = document.createElement("li");
      const title = document.createElement("strong");
      const detailList = document.createElement("dl");
      title.textContent = server.name;
      appendUuidRow(detailList, server.server_uuid);
      item.append(title, detailList);
      list.appendChild(item);
    });
  }

  function renderArchivedServerStats(root, archives) {
    const list = root.querySelector(".statsArchivedServerData");
    if (!list) return;
    list.replaceChildren();

    if (!archives.length) {
      const item = document.createElement("li");
      item.textContent = "No retained stats for removed servers";
      list.appendChild(item);
      return;
    }

    archives.forEach((archive) => {
      const item = document.createElement("li");
      const title = document.createElement("strong");
      const actions = document.createElement("div");
      const detailList = document.createElement("dl");
      const deleteButton = document.createElement("button");

      title.textContent = archive.name || "Removed server";
      actions.className = "statsArchiveActions";
      deleteButton.type = "button";
      deleteButton.textContent = "Delete";
      deleteButton.title = "Delete retained stats";
      deleteButton.addEventListener("click", () => {
        app.sendSocketMessage({
          type: RSC.messages.deleteArchivedServerStats,
          server_uuid: archive.server_uuid,
        });
      });
      actions.appendChild(deleteButton);

      appendUuidRow(detailList, archive.server_uuid);
      [
        ["Specialization", archive.specialization || "Unknown"],
        ["Observed Names", archive.observed_names ?? 0],
        ["Last Seen", formatTimestamp(archive.last_seen_at)],
      ].forEach(([label, value]) => {
        const term = document.createElement("dt");
        const description = document.createElement("dd");
        term.textContent = label;
        description.textContent = value;
        detailList.append(term, description);
      });

      item.append(title, actions, detailList);
      const timeframeContainer = document.createElement("div");
      renderTimeframeStats(timeframeContainer, archive.stats);
      item.appendChild(timeframeContainer);
      const sessionsContainer = document.createElement("div");
      renderRecentSessions(sessionsContainer, archive.recent_sessions);
      item.appendChild(sessionsContainer);
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
      <section class="statsDetails statsDistribution">
        <h2>Server Mix</h2>
        <ul class="statsSpecializationDistribution"></ul>
      </section>
      <section class="statsDetails">
        <h2>Online Players</h2>
        <ul class="statsOnlinePlayers"></ul>
      </section>
      <section class="statsDetails">
        <h2>User Activity</h2>
        <p class="accountMeta">Every known player across all Minecraft servers this controller manages, and which server(s) they're on.</p>
        <ul class="statsUserActivity"></ul>
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
        <h2>Server Data</h2>
        <ul class="statsServerData"></ul>
      </section>
      <section class="statsDetails">
        <h2>Retained Server Data</h2>
        <ul class="statsArchivedServerData"></ul>
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

    // Every refresh rebuilds the timeframe/archive DOM from scratch, so any
    // ResizeObservers watching last cycle's canvases are now watching
    // detached elements - tear them down before creating this cycle's set.
    disconnectChartResizeObservers();

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
    renderSpecializationDistribution(root, stats);
    renderOnlinePlayers(root, stats.onlinePlayers);
    renderGlobalUserActivity(root, globalPlayerActivity());
    renderSpecializationStats(root, servers);
    renderServerData(root, servers);
    renderArchivedServerStats(root, archivedServerStats());
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
