window.RSCApp = window.RSCApp || {};

(function (app) {
  const RSC = window.RSC_CONSTANTS;

  function config() {
    return window.serverInfoObj?.config || window.config || {};
  }

  function cloneConfig() {
    return JSON.parse(JSON.stringify(config()));
  }

  function groupList(cfg) {
    if (!Array.isArray(cfg.minecraft_account_filter_detail_groups)) {
      cfg.minecraft_account_filter_detail_groups = [];
    }
    return cfg.minecraft_account_filter_detail_groups;
  }

  function groupUuid() {
    return crypto.randomUUID ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`;
  }

  function sendConfig(cfg) {
    app.sendSocketMessage({
      type: RSC.messages.configChange,
      updatedConfig: cfg,
    });
    app.setEditorConfig?.(cfg);
    window.serverInfoObj = window.serverInfoObj || {};
    window.serverInfoObj.config = cfg;
    app.updateAdministration?.();
  }

  function minecraftServers(cfg) {
    return Array.isArray(cfg.servers)
      ? cfg.servers.filter((server) => server.specialized_server_type === "Minecraft")
      : [];
  }

  function ensureGroupMembership(server) {
    server.specialization_options = server.specialization_options || {};
    if (!Array.isArray(server.specialization_options.account_filter_groups)) {
      server.specialization_options.account_filter_groups = [];
    }
    return server.specialization_options.account_filter_groups;
  }

  function setServerMembership(cfg, serverName, groupId, enabled) {
    const server = minecraftServers(cfg).find((candidate) => candidate.name === serverName);
    if (!server) return;
    const groups = ensureGroupMembership(server);
    const index = groups.indexOf(groupId);
    if (enabled && index === -1) groups.push(groupId);
    if (!enabled && index !== -1) groups.splice(index, 1);
  }

  function localDateFromDatetimeLocal(value) {
    const match = String(value || "").match(
      /^([+-]?\d{4,})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/,
    );
    if (!match) return null;
    const date = new Date(0);
    date.setFullYear(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
    date.setHours(Number(match[4]), Number(match[5]), Number(match[6] || "0"), 0);
    return Number.isNaN(date.getTime()) ? null : date;
  }

  function isoYear(year) {
    if (year >= 0 && year <= 9999) return String(year).padStart(4, "0");
    const sign = year < 0 ? "-" : "+";
    return `${sign}${String(Math.abs(year)).padStart(6, "0")}`;
  }

  function datetimeLocalYear(year) {
    if (year >= 0) return String(year).padStart(4, "0");
    return `-${String(Math.abs(year)).padStart(6, "0")}`;
  }

  function formatIsoOffset(offsetMinutes) {
    const sign = offsetMinutes >= 0 ? "+" : "-";
    const abs = Math.abs(offsetMinutes);
    const hours = String(Math.floor(abs / 60)).padStart(2, "0");
    const minutes = String(abs % 60).padStart(2, "0");
    return `${sign}${hours}:${minutes}`;
  }

  function dateToIso8601(date) {
    const offsetMinutes = -date.getTimezoneOffset();
    const year = isoYear(date.getFullYear());
    const month = String(date.getMonth() + 1).padStart(2, "0");
    const day = String(date.getDate()).padStart(2, "0");
    const hours = String(date.getHours()).padStart(2, "0");
    const minutes = String(date.getMinutes()).padStart(2, "0");
    const seconds = String(date.getSeconds()).padStart(2, "0");
    return `${year}-${month}-${day}T${hours}:${minutes}:${seconds}${formatIsoOffset(offsetMinutes)}`;
  }

  function nowIso8601Timestamp() {
    return dateToIso8601(new Date());
  }

  function parseExpirationDate(value) {
    if (!value || value === "forever") return null;
    const text = String(value).trim();
    const legacy = text.match(
      /^(\d{4,})-(\d{2})-(\d{2}) (\d{2}):(\d{2})(?::(\d{2}))?(?: ([+-])(\d{2})(\d{2}))?$/,
    );
    if (legacy) {
      const offset = legacy[7] ? `${legacy[7]}${legacy[8]}:${legacy[9]}` : formatIsoOffset(-new Date().getTimezoneOffset());
      return new Date(`${legacy[1]}-${legacy[2]}-${legacy[3]}T${legacy[4]}:${legacy[5]}:${legacy[6] || "00"}${offset}`);
    }
    const date = new Date(text);
    return Number.isNaN(date.getTime()) ? null : date;
  }

  function expirationToDatetimeLocal(value) {
    if (!value || value === "forever") return "";
    const date = parseExpirationDate(value);
    if (!date) return "";
    return `${datetimeLocalYear(date.getFullYear())}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}T${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}:${String(date.getSeconds()).padStart(2, "0")}`;
  }

  function datetimeLocalToIso8601(value) {
    if (!value) return "forever";
    const date = localDateFromDatetimeLocal(value);
    return date ? dateToIso8601(date) : "forever";
  }

  function formatExpirationDate(date) {
    const month = String(date.getMonth() + 1).padStart(2, "0");
    const day = String(date.getDate()).padStart(2, "0");
    let hour = date.getHours() % 12;
    if (hour === 0) hour = 12;
    return `${month} ${day}, ${isoYear(date.getFullYear())} at ${String(hour).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}:${String(date.getSeconds()).padStart(2, "0")} ${date.getHours() >= 12 ? "PM" : "AM"}`;
  }

  function addCalendarMonths(date, months) {
    const next = new Date(date.getTime());
    const originalDay = next.getDate();
    next.setDate(1);
    next.setMonth(next.getMonth() + months);
    const maxDay = new Date(next.getFullYear(), next.getMonth() + 1, 0).getDate();
    next.setDate(Math.min(originalDay, maxDay));
    return next;
  }

  function pluralUnit(value, unit) {
    return `${value} ${unit}${value === 1 ? "" : "s"}`;
  }

  function formatTimeLeft(expiresAt, now = new Date()) {
    if (expiresAt <= now) return "00:00:00";
    let months =
      (expiresAt.getFullYear() - now.getFullYear()) * 12 +
      (expiresAt.getMonth() - now.getMonth());
    while (months > 0 && addCalendarMonths(now, months) > expiresAt) months -= 1;
    while (addCalendarMonths(now, months + 1) <= expiresAt) months += 1;
    let cursor = addCalendarMonths(now, months);
    let remaining = expiresAt.getTime() - cursor.getTime();
    const days = Math.floor(remaining / 86400000);
    remaining -= days * 86400000;
    const hours = Math.floor(remaining / 3600000);
    remaining -= hours * 3600000;
    const minutes = Math.floor(remaining / 60000);
    remaining -= minutes * 60000;
    const seconds = Math.floor(remaining / 1000);
    const parts = [];
    if (months > 0) parts.push(pluralUnit(months, "Month"));
    if (days > 0) parts.push(pluralUnit(days, "Day"));
    parts.push(`${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`);
    return parts.join(" ");
  }

  function expirationSummary(value) {
    const date = parseExpirationDate(value);
    if (!date) return "";
    return `Expires on ${formatExpirationDate(date)} (${formatTimeLeft(date)})`;
  }

  function setExpirationSummary(element, value) {
    element.dataset.expires = value || "";
    element.textContent = expirationSummary(value);
  }

  function entryName(entry, isIpBan) {
    return isIpBan ? entry.ip || "" : entry.name || "";
  }

  function setEntryName(entry, isIpBan, value) {
    entry[isIpBan ? "ip" : "name"] = value;
  }

  function entryIsPermanent(entry) {
    return !entry.expires || entry.expires === "forever";
  }

  function applyExpiry(entry, permanent, expiresValue) {
    entry.expires = permanent ? "forever" : datetimeLocalToIso8601(expiresValue);
  }

  function setExpirationVisible(expirationField, permanent) {
    expirationField.hidden = permanent;
  }

  function newAccountEntry(name, isIpBan, permanent, expiresValue, reason) {
    const entry = {
      created: nowIso8601Timestamp(),
      source: "RustServerController",
      expires: permanent ? "forever" : datetimeLocalToIso8601(expiresValue),
      reason: reason || null,
    };
    setEntryName(entry, isIpBan, name);
    return entry;
  }

  function updateGroupEntry(groupId, field, index, updater) {
    const cfg = cloneConfig();
    const targetGroup = groupList(cfg).find((item) => item.uuid === groupId);
    const targetEntry = targetGroup?.[field]?.[index];
    if (!targetEntry) return;
    updater(targetEntry, targetGroup, cfg);
    sendConfig(cfg);
  }

  function renderEntryList(container, group, field, labels) {
    const entries = Array.isArray(group[field]) ? group[field] : (group[field] = []);
    const list = document.createElement("div");
    const addRow = document.createElement("div");
    const nameInput = document.createElement("input");
    const permanentInput = document.createElement("input");
    const expiresInput = document.createElement("input");
    const addButton = document.createElement("button");
    const isBan = field === "ban_list" || field === "banned_ips";
    const isIpBan = field === "banned_ips";
    const supportsTemporary = field === "whitelist" || isBan;

    list.className = "adminEntryList";
    entries.forEach((entry, index) => {
      const row = document.createElement("div");
      const primary = document.createElement("div");
      const secondary = document.createElement("div");
      const name = document.createElement("p");
      const expirationField = document.createElement("label");
      const expirationText = document.createElement("span");
      const expirationStatus = document.createElement("p");
      const permanentLabel = document.createElement("label");
      const permanent = document.createElement("input");
      const expires = document.createElement("input");
      const remove = document.createElement("button");
      const ban = document.createElement("button");

      row.className = isBan ? "adminEntryCard adminBanEntryCard" : "adminEntryCard";
      primary.className = "adminEntryPrimary";
      secondary.className = "adminEntrySecondary";
      name.className = "adminEntryName";
      name.textContent = entryName(entry, isIpBan);
      expirationField.className = "adminExpirationField";
      expirationText.textContent = "Expiration";
      expirationStatus.className = "adminExpirationStatus";
      setExpirationSummary(expirationStatus, entry.expires);
      expirationStatus.hidden = entryIsPermanent(entry);
      permanent.type = "checkbox";
      permanent.checked = entryIsPermanent(entry);
      permanentLabel.className = "adminPermanentToggle";
      permanentLabel.append(permanent, document.createTextNode("Permanent"));
      expires.type = "datetime-local";
      expires.value = expirationToDatetimeLocal(entry.expires);
      expires.disabled = permanent.checked;
      expirationField.append(expirationText, expires);
      setExpirationVisible(expirationField, permanent.checked);
      remove.type = "button";
      remove.textContent = isBan ? "Unban" : "Remove";
      ban.type = "button";
      ban.textContent = "Ban";

      permanent.addEventListener("change", () => {
        expires.disabled = permanent.checked;
        setExpirationVisible(expirationField, permanent.checked);
        expirationStatus.hidden = permanent.checked;
        setExpirationSummary(
          expirationStatus,
          permanent.checked ? "" : datetimeLocalToIso8601(expires.value),
        );
        updateGroupEntry(group.uuid, field, index, (target) => {
          applyExpiry(target, permanent.checked, expires.value);
        });
      });
      expires.addEventListener("change", () => {
        setExpirationSummary(expirationStatus, datetimeLocalToIso8601(expires.value));
        updateGroupEntry(group.uuid, field, index, (target) => {
          applyExpiry(target, permanent.checked, expires.value);
        });
      });
      remove.addEventListener("click", () => {
        const cfg = cloneConfig();
        groupList(cfg).find((item) => item.uuid === group.uuid)[field].splice(index, 1);
        sendConfig(cfg);
      });
      ban.addEventListener("click", () => {
        const cfg = cloneConfig();
        const targetGroup = groupList(cfg).find((item) => item.uuid === group.uuid);
        const sourceEntry = targetGroup?.whitelist?.[index];
        if (!sourceEntry) return;
        const bannedName = entryName(sourceEntry, false).trim();
        if (!bannedName) return;
        targetGroup.whitelist.splice(index, 1);
        targetGroup.ban_list = Array.isArray(targetGroup.ban_list) ? targetGroup.ban_list : [];
        targetGroup.ban_list.push(
          newAccountEntry(bannedName, false, true, "", "Banned by administrator"),
        );
        sendConfig(cfg);
      });

      primary.appendChild(name);
      if (!isBan && !isIpBan) primary.appendChild(ban);
      primary.appendChild(remove);
      if (supportsTemporary && (!isBan || !entryIsPermanent(entry))) {
        secondary.appendChild(expirationStatus);
      }
      if (supportsTemporary && !isBan) {
        secondary.append(expirationField, permanentLabel);
      }
      row.appendChild(primary);
      if (secondary.childElementCount > 0) row.appendChild(secondary);
      list.appendChild(row);
    });

    addRow.className = isBan ? "adminEntryCard adminEntryAddCard adminBanEntryCard" : "adminEntryCard adminEntryAddCard";
    nameInput.placeholder = labels.name;
    const expirationField = document.createElement("label");
    const expirationText = document.createElement("span");
    expirationField.className = "adminExpirationField";
    expirationText.textContent = "Expiration";
    permanentInput.type = "checkbox";
    permanentInput.checked = true;
    expiresInput.type = "datetime-local";
    expiresInput.disabled = true;
    expirationField.append(expirationText, expiresInput);
    setExpirationVisible(expirationField, permanentInput.checked);
    const permanentLabel = document.createElement("label");
    permanentLabel.className = "adminPermanentToggle";
    permanentLabel.append(permanentInput, document.createTextNode("Permanent"));
    permanentInput.addEventListener("change", () => {
      expiresInput.disabled = permanentInput.checked;
      setExpirationVisible(expirationField, permanentInput.checked);
    });
    addButton.type = "button";
    addButton.textContent = "Add";
    addButton.addEventListener("click", () => {
      const name = nameInput.value.trim();
      if (!name) return;
      const cfg = cloneConfig();
      const target = groupList(cfg).find((item) => item.uuid === group.uuid);
      target[field] = Array.isArray(target[field]) ? target[field] : [];
      const entry = newAccountEntry(
        name,
        isIpBan,
        permanentInput.checked,
        expiresInput.value,
        isBan ? "Banned by administrator" : null,
      );
      if (!isBan) delete entry.reason;
      target[field].push(entry);
      sendConfig(cfg);
    });
    const addPrimary = document.createElement("div");
    const addSecondary = document.createElement("div");
    addPrimary.className = "adminEntryPrimary";
    addSecondary.className = "adminEntrySecondary";
    addPrimary.append(nameInput, addButton);
    if (supportsTemporary) {
      addSecondary.append(expirationField, permanentLabel);
    }
    addRow.append(addPrimary, addSecondary);

    container.append(addRow, list);
  }

  function renderGroup(container, group, cfg) {
    const card = document.createElement("section");
    const header = document.createElement("div");
    const title = document.createElement("input");
    const uuid = document.createElement("button");
    const remove = document.createElement("button");
    const serverPanel = document.createElement("div");
    const whitelist = document.createElement("div");
    const bans = document.createElement("div");
    const ipBans = document.createElement("div");

    card.className = "adminGroup";
    header.className = "adminGroupHeader";
    title.value = group.name || "";
    title.placeholder = "Group name";
    uuid.type = "button";
    uuid.textContent = group.uuid || "";
    uuid.title = "Copy group UUID";
    uuid.addEventListener("click", () => navigator.clipboard?.writeText(group.uuid || ""));
    remove.type = "button";
    remove.textContent = "Delete";
    remove.addEventListener("click", () => {
      const next = cloneConfig();
      next.minecraft_account_filter_detail_groups = groupList(next).filter(
        (item) => item.uuid !== group.uuid,
      );
      minecraftServers(next).forEach((server) => {
        const groups = ensureGroupMembership(server);
        const index = groups.indexOf(group.uuid);
        if (index !== -1) groups.splice(index, 1);
      });
      sendConfig(next);
    });
    title.addEventListener("change", () => {
      const next = cloneConfig();
      groupList(next).find((item) => item.uuid === group.uuid).name = title.value.trim();
      sendConfig(next);
    });
    header.append(title, uuid, remove);

    serverPanel.className = "adminServers";
    minecraftServers(cfg).forEach((server) => {
      const label = document.createElement("label");
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.checked = ensureGroupMembership(server).includes(group.uuid);
      checkbox.addEventListener("change", () => {
        const next = cloneConfig();
        setServerMembership(next, server.name, group.uuid, checkbox.checked);
        sendConfig(next);
      });
      label.append(checkbox, document.createTextNode(server.name));
      serverPanel.appendChild(label);
    });

    whitelist.innerHTML = "<h3>Whitelist</h3>";
    renderEntryList(whitelist, group, "whitelist", { name: "Minecraft name" });
    bans.innerHTML = "<h3>Ban List</h3>";
    renderEntryList(bans, group, "ban_list", { name: "Minecraft name" });
    ipBans.innerHTML = "<h3>IP Ban List</h3>";
    renderEntryList(ipBans, group, "banned_ips", { name: "IP address" });

    card.append(header, serverPanel, whitelist, bans, ipBans);
    container.appendChild(card);
  }

  function ensureMarkup() {
    const root = document.querySelector(".administrationDashboard");
    if (!root) return null;
    root.innerHTML = `
      <section class="adminToolbar">
        <input class="adminNewGroupName" placeholder="New group name">
        <button type="button" class="adminCreateGroup">Create Group</button>
      </section>
      <section class="adminGroups"></section>
    `;
    root.querySelector(".adminCreateGroup").addEventListener("click", () => {
      const input = root.querySelector(".adminNewGroupName");
      const name = input.value.trim();
      if (!name) return;
      const cfg = cloneConfig();
      groupList(cfg).push({
        name,
        uuid: groupUuid(),
        whitelist: [],
        ban_list: [],
        banned_ips: [],
      });
      sendConfig(cfg);
    });
    return root;
  }

  app.updateAdministration = function () {
    const root = ensureMarkup();
    if (!root) return;
    const cfg = config();
    const groups = root.querySelector(".adminGroups");
    groupList(cfg).forEach((group) => renderGroup(groups, group, cfg));
  };

  let expirationTicker = null;

  app.initAdministration = function () {
    app.updateAdministration();
    if (!expirationTicker) {
      expirationTicker = setInterval(() => {
        document
          .querySelectorAll(".administrationDashboard .adminExpirationStatus[data-expires]")
          .forEach((element) => {
            element.textContent = expirationSummary(element.dataset.expires);
          });
      }, 1000);
    }
  };
})(window.RSCApp);
