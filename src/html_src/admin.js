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

  function nowMinecraftTimestamp() {
    const offsetMinutes = -new Date().getTimezoneOffset();
    const sign = offsetMinutes >= 0 ? "+" : "-";
    const abs = Math.abs(offsetMinutes);
    const hours = String(Math.floor(abs / 60)).padStart(2, "0");
    const minutes = String(abs % 60).padStart(2, "0");
    const local = new Date(Date.now() - new Date().getTimezoneOffset() * 60000)
      .toISOString()
      .slice(0, 19)
      .replace("T", " ");
    return `${local} ${sign}${hours}${minutes}`;
  }

  function minecraftTimestampToDatetimeLocal(value) {
    if (!value || value === "forever") return "";
    const match = String(value).match(
      /^(\d{4}-\d{2}-\d{2}) (\d{2}:\d{2})(?::\d{2})?(?: [+-]\d{4})?$/,
    );
    return match ? `${match[1]}T${match[2]}` : "";
  }

  function datetimeLocalToMinecraftTimestamp(value) {
    if (!value) return "forever";
    const offsetMinutes = -new Date().getTimezoneOffset();
    const sign = offsetMinutes >= 0 ? "+" : "-";
    const abs = Math.abs(offsetMinutes);
    const hours = String(Math.floor(abs / 60)).padStart(2, "0");
    const minutes = String(abs % 60).padStart(2, "0");
    return `${value.replace("T", " ")}:00 ${sign}${hours}${minutes}`;
  }

  function renderEntryList(container, group, field, labels) {
    const entries = Array.isArray(group[field]) ? group[field] : (group[field] = []);
    const list = document.createElement("div");
    const addRow = document.createElement("div");
    const nameInput = document.createElement("input");
    const permanentInput = document.createElement("input");
    const expiresInput = document.createElement("input");
    const reasonInput = document.createElement("input");
    const addButton = document.createElement("button");
    const isBan = field === "ban_list" || field === "banned_ips";
    const isIpBan = field === "banned_ips";

    list.className = "adminEntryList";
    entries.forEach((entry, index) => {
      const row = document.createElement("div");
      const name = document.createElement("input");
      const permanentLabel = document.createElement("label");
      const permanent = document.createElement("input");
      const expires = document.createElement("input");
      const reason = document.createElement("input");
      const remove = document.createElement("button");

      row.className = isBan ? "adminEntryRow adminBanEntryRow" : "adminEntryRow";
      name.value = isIpBan ? entry.ip || "" : entry.name || "";
      name.placeholder = labels.name;
      permanent.type = "checkbox";
      permanent.checked = !entry.expires || entry.expires === "forever";
      permanentLabel.className = "adminPermanentToggle";
      permanentLabel.append(permanent, document.createTextNode("Permanent"));
      expires.type = "datetime-local";
      expires.value = minecraftTimestampToDatetimeLocal(entry.expires);
      expires.disabled = permanent.checked;
      reason.value = entry.reason || "";
      reason.placeholder = "Reason";
      [permanentLabel, expires, reason].forEach((input) => {
        input.style.display = isBan ? "" : "none";
      });
      remove.type = "button";
      remove.textContent = "Remove";

      name.addEventListener("change", () => {
        const cfg = cloneConfig();
        const target = groupList(cfg).find((item) => item.uuid === group.uuid)[field][index];
        target[isIpBan ? "ip" : "name"] = name.value.trim();
        sendConfig(cfg);
      });
      [
        ["reason", reason],
      ].forEach(([key, input]) => {
        input.addEventListener("change", () => {
          const cfg = cloneConfig();
          groupList(cfg).find((item) => item.uuid === group.uuid)[field][index][key] =
            input.value.trim() || null;
          sendConfig(cfg);
        });
      });
      permanent.addEventListener("change", () => {
        const cfg = cloneConfig();
        const target = groupList(cfg).find((item) => item.uuid === group.uuid)[field][index];
        target.expires = permanent.checked
          ? "forever"
          : datetimeLocalToMinecraftTimestamp(expires.value);
        sendConfig(cfg);
      });
      expires.addEventListener("change", () => {
        const cfg = cloneConfig();
        const target = groupList(cfg).find((item) => item.uuid === group.uuid)[field][index];
        target.expires = datetimeLocalToMinecraftTimestamp(expires.value);
        sendConfig(cfg);
      });
      remove.addEventListener("click", () => {
        const cfg = cloneConfig();
        groupList(cfg).find((item) => item.uuid === group.uuid)[field].splice(index, 1);
        sendConfig(cfg);
      });

      row.appendChild(name);
      if (isBan) row.append(permanentLabel, expires, reason);
      row.appendChild(remove);
      list.appendChild(row);
    });

    addRow.className = isBan ? "adminEntryRow adminBanEntryRow" : "adminEntryRow";
    nameInput.placeholder = labels.name;
    permanentInput.type = "checkbox";
    permanentInput.checked = true;
    expiresInput.type = "datetime-local";
    expiresInput.disabled = true;
    reasonInput.placeholder = "Reason";
    const permanentLabel = document.createElement("label");
    permanentLabel.className = "adminPermanentToggle";
    permanentLabel.append(permanentInput, document.createTextNode("Permanent"));
    [permanentLabel, expiresInput, reasonInput].forEach((input) => {
      input.style.display = isBan ? "" : "none";
    });
    permanentInput.addEventListener("change", () => {
      expiresInput.disabled = permanentInput.checked;
    });
    addButton.type = "button";
    addButton.textContent = "Add";
    addButton.addEventListener("click", () => {
      const name = nameInput.value.trim();
      if (!name) return;
      const cfg = cloneConfig();
      const target = groupList(cfg).find((item) => item.uuid === group.uuid);
      const entry = {
        [isIpBan ? "ip" : "name"]: name,
        created: nowMinecraftTimestamp(),
        source: "RustServerController",
        expires: permanentInput.checked
          ? "forever"
          : datetimeLocalToMinecraftTimestamp(expiresInput.value),
        reason: reasonInput.value.trim() || null,
      };
      if (!isBan) {
        delete entry.created;
        delete entry.source;
        delete entry.expires;
        delete entry.reason;
      }
      target[field].push(entry);
      sendConfig(cfg);
    });
    addRow.appendChild(nameInput);
    if (isBan) addRow.append(permanentLabel, expiresInput, reasonInput);
    addRow.appendChild(addButton);

    container.append(list, addRow);
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

  app.initAdministration = function () {
    app.updateAdministration();
  };
})(window.RSCApp);
