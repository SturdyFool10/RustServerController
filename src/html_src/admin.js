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

  const accountPermissions = ["view", "control", "config", "stats", "console", "admin"];
  let accountPanelRequest = 0;

  function bytesToBase64(bytes) {
    let binary = "";
    bytes.forEach((byte) => {
      binary += String.fromCharCode(byte);
    });
    return btoa(binary);
  }

  async function derivePasswordHash(password, salt) {
    const encoder = new TextEncoder();
    const key = await crypto.subtle.importKey(
      "raw",
      encoder.encode(password),
      "PBKDF2",
      false,
      ["deriveBits"],
    );
    const bits = await crypto.subtle.deriveBits(
      {
        name: "PBKDF2",
        salt: encoder.encode(salt),
        iterations: 310000,
        hash: "SHA-256",
      },
      key,
      256,
    );
    return bytesToBase64(new Uint8Array(bits));
  }

  async function createPasswordPayload(password) {
    const saltBytes = new Uint8Array(32);
    crypto.getRandomValues(saltBytes);
    const password_salt = bytesToBase64(saltBytes);
    return {
      password_salt,
      password_hash: await derivePasswordHash(password, password_salt),
    };
  }

  function selectedPermissions(container) {
    return Array.from(container.querySelectorAll("input[type='checkbox']:checked")).map(
      (input) => input.value,
    );
  }

  function selectedPermissionDecisions(container) {
    return Array.from(container.querySelectorAll("[data-permission-state]"))
      .map((select) => ({
        permission: select.dataset.permission,
        state: select.dataset.permissionState,
      }))
      .filter((decision) => decision.permission);
  }

  function selectedGroups(container) {
    return Array.from(container.querySelectorAll("input[data-group]:checked")).map(
      (input) => input.dataset.group,
    );
  }

  function decisionState(decisions, permission) {
    return (
      (decisions || []).find((decision) => decision.permission === permission)?.state || "default"
    );
  }

  function renderDecisionSelect(permission, decisions = []) {
    const control = document.createElement("div");
    control.className = "permissionDecisionControl";
    control.dataset.permission = permission;
    control.dataset.permissionState = decisionState(decisions, permission);
    ["default", "granted", "blocked"].forEach((state) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = `permissionDecision permissionDecision-${state}`;
      button.dataset.state = state;
      button.textContent = state[0].toUpperCase();
      button.title = state[0].toUpperCase() + state.slice(1);
      button.setAttribute("aria-label", `${permission} ${button.title}`);
      button.addEventListener("click", () => {
        control.dataset.permissionState = state;
        control.querySelectorAll(".permissionDecision").forEach((item) => {
          item.classList.toggle("selected", item.dataset.state === state);
        });
      });
      control.appendChild(button);
    });
    control.querySelectorAll(".permissionDecision").forEach((item) => {
      item.classList.toggle("selected", item.dataset.state === control.dataset.permissionState);
    });
    return control;
  }

  function renderPermissionPicker(decisions = [], groups = [], availableGroups = []) {
    const picker = document.createElement("div");
    const global = document.createElement("div");
    const serverList = document.createElement("div");
    const groupList = document.createElement("div");
    picker.className = "accountPermissionPicker";
    global.className = "accountPermissionGlobal";
    accountPermissions.forEach((permission) => {
      const label = document.createElement("label");
      label.className = "permissionDecisionLabel";
      label.append(document.createTextNode(permission), renderDecisionSelect(permission, decisions));
      global.appendChild(label);
    });
    groupList.className = "accountGroupPicker";
    availableGroups.forEach((group) => {
      const label = document.createElement("label");
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.dataset.group = group.name;
      checkbox.checked = groups.includes(group.name);
      label.append(checkbox, document.createTextNode(group.name));
      groupList.appendChild(label);
    });
    serverList.className = "accountServerPermissionList";
    (config().servers || []).forEach((server) => {
      const serverId = server.server_uuid || server.name;
      if (!serverId || !server.name) return;
      const row = document.createElement("div");
      const name = document.createElement("p");
      row.className = "accountServerPermissionRow";
      name.textContent = server.name;
      row.appendChild(name);
      ["view", "control", "config", "stats", "console"].forEach((permission) => {
        const value = `server:${serverId}:${permission}`;
        const label = document.createElement("label");
        label.className = "permissionDecisionLabel";
        label.append(document.createTextNode(permission), renderDecisionSelect(value, decisions));
        row.appendChild(label);
      });
      serverList.appendChild(row);
    });
    picker.append(global, groupList, serverList);
    return picker;
  }

  function legacyPermissionsToDecisions(permissions = []) {
    return permissions.map((permission) => ({ permission, state: "granted" }));
  }

  function formatRequestedAt(value) {
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? "" : date.toLocaleString();
  }

  function renderPermissionModelEditor(accounts) {
    const section = document.createElement("section");
    const title = document.createElement("h3");
    const defaultsTitle = document.createElement("p");
    const defaultPicker = renderPermissionPicker(accounts.default_permissions || [], [], []);
    const groupsTitle = document.createElement("p");
    const groupList = document.createElement("div");
    const newGroupName = document.createElement("input");
    const addGroup = document.createElement("button");
    const save = document.createElement("button");
    const status = document.createElement("p");

    section.className = "accountModelEditor";
    title.textContent = "Permission Model";
    defaultsTitle.className = "accountMeta";
    defaultsTitle.textContent =
      "Hierarchy: User overrides -> Groups -> Defaults. Defaults make observers: view, stats, and console are granted; modification/admin permissions are blocked.";
    groupsTitle.className = "accountMeta";
    groupsTitle.textContent = "Groups inherit from Defaults unless a group grants or blocks a permission.";
    groupList.className = "accountList";
    status.className = "accountPanelStatus";
    newGroupName.placeholder = "New group name";
    addGroup.type = "button";
    addGroup.textContent = "Add Group";
    save.type = "button";
    save.textContent = "Save Permission Model";

    const groups = JSON.parse(JSON.stringify(accounts.groups || []));
    function drawGroups() {
      groupList.replaceChildren();
      groups.forEach((group, index) => {
        const row = document.createElement("div");
        const name = document.createElement("input");
        const picker = renderPermissionPicker(group.permissions || [], [], []);
        const remove = document.createElement("button");
        row.className = "accountCard";
        name.value = group.name;
        remove.type = "button";
        remove.textContent = "Remove";
        name.addEventListener("change", () => {
          group.name = name.value.trim();
        });
        remove.addEventListener("click", () => {
          groups.splice(index, 1);
          drawGroups();
        });
        row.append(name, picker, remove);
        groupList.appendChild(row);
      });
    }
    drawGroups();

    addGroup.addEventListener("click", () => {
      const name = newGroupName.value.trim();
      if (!name) return;
      groups.push({ name, permissions: [] });
      newGroupName.value = "";
      drawGroups();
    });
    save.addEventListener("click", async () => {
      save.disabled = true;
      status.textContent = "";
      try {
        const nextGroups = Array.from(groupList.querySelectorAll(".accountCard")).map((row, index) => ({
          name: row.querySelector("input")?.value.trim() || groups[index]?.name || "",
          permissions: selectedPermissionDecisions(row),
        })).filter((group) => group.name);
        await app.authRequest("/auth/admin/update-permission-model", {
          default_permissions: selectedPermissionDecisions(defaultPicker),
          groups: nextGroups,
        });
        await refreshAccountAdministration();
      } catch (error) {
        status.textContent = error.message;
      } finally {
        save.disabled = false;
      }
    });

    section.append(
      title,
      defaultsTitle,
      defaultPicker,
      groupsTitle,
      groupList,
      newGroupName,
      addGroup,
      save,
      status,
    );
    return section;
  }

  async function refreshAccountAdministration() {
    app.updateAdministration?.();
  }

  async function renderAccountAdministration(panel) {
    if (!app.hasPermission?.("admin")) {
      panel.remove();
      return;
    }
    const requestId = ++accountPanelRequest;
    panel.innerHTML = "<h2>Accounts</h2><p class=\"accountPanelStatus\">Loading accounts...</p>";
    let accounts;
    try {
      accounts = await app.authRequest("/auth/accounts");
    } catch (error) {
      panel.innerHTML = "<h2>Accounts</h2>";
      const status = document.createElement("p");
      status.className = "accountPanelStatus";
      status.textContent = error.message;
      panel.appendChild(status);
      return;
    }
    if (requestId !== accountPanelRequest || !panel.isConnected) return;

    panel.innerHTML = "";
    const title = document.createElement("h2");
    const createCard = document.createElement("form");
    const username = document.createElement("input");
    const password = document.createElement("input");
    const repeat = document.createElement("input");
    const permissions = renderPermissionPicker([], [], accounts.groups || []);
    const submit = document.createElement("button");
    const reset = document.createElement("button");
    const status = document.createElement("p");
    const requestsTitle = document.createElement("h3");
    const requests = document.createElement("div");
    const usersTitle = document.createElement("h3");
    const users = document.createElement("div");

    title.textContent = "Accounts";
    createCard.className = "accountCreateCard";
    username.placeholder = "Username";
    username.required = true;
    password.type = "password";
    password.placeholder = "Password";
    password.autocomplete = "new-password";
    password.required = true;
    repeat.type = "password";
    repeat.placeholder = "Repeat password";
    repeat.autocomplete = "new-password";
    repeat.required = true;
    submit.type = "submit";
    submit.textContent = "Create Account";
    reset.type = "button";
    reset.className = "accountDangerButton";
    reset.textContent = "Reset Credential Stores";
    status.className = "accountPanelStatus";
    createCard.append(username, password, repeat, permissions, submit, reset, status);
    createCard.addEventListener("submit", async (event) => {
      event.preventDefault();
      status.textContent = "";
      submit.disabled = true;
      try {
        if (password.value.length < 8) throw new Error("password must be at least 8 characters");
        if (password.value !== repeat.value) throw new Error("passwords do not match");
        await app.authRequest("/auth/admin/create-user", {
          username: username.value.trim(),
          ...(await createPasswordPayload(password.value)),
          permission_overrides: selectedPermissionDecisions(permissions),
          groups: selectedGroups(permissions),
        });
        await refreshAccountAdministration();
      } catch (error) {
        status.textContent = error.message;
      } finally {
        submit.disabled = false;
      }
    });
    reset.addEventListener("click", async () => {
      const confirmed = window.confirm(
        "Reset all credential stores, auth users, pending requests, OAuth clients, and active sessions?",
      );
      if (!confirmed) return;
      reset.disabled = true;
      status.textContent = "";
      try {
        await app.authRequest("/auth/admin/reset-credential-stores", {});
        window.location.reload();
      } catch (error) {
        status.textContent = error.message;
        reset.disabled = false;
      }
    });

    requestsTitle.textContent = "Account Requests";
    requests.className = "accountList";
    (accounts.requests || []).forEach((request) => {
      const row = document.createElement("div");
      const name = document.createElement("p");
      const requestedAt = document.createElement("p");
      const picker = renderPermissionPicker([], [], accounts.groups || []);
      const accept = document.createElement("button");
      const reject = document.createElement("button");
      row.className = "accountCard";
      name.className = "accountName";
      name.textContent = request.username;
      requestedAt.className = "accountMeta";
      requestedAt.textContent = formatRequestedAt(request.requested_at);
      accept.type = "button";
      accept.textContent = "Accept";
      reject.type = "button";
      reject.textContent = "Reject";
      accept.addEventListener("click", async () => {
        accept.disabled = true;
        try {
          await app.authRequest("/auth/admin/approve-request", {
            username: request.username,
            permission_overrides: selectedPermissionDecisions(picker),
            groups: selectedGroups(picker),
          });
          await refreshAccountAdministration();
        } catch (error) {
          requestedAt.textContent = error.message;
        } finally {
          accept.disabled = false;
        }
      });
      reject.addEventListener("click", async () => {
        reject.disabled = true;
        try {
          await app.authRequest("/auth/admin/reject-request", { username: request.username });
          await refreshAccountAdministration();
        } catch (error) {
          requestedAt.textContent = error.message;
        } finally {
          reject.disabled = false;
        }
      });
      row.append(name, requestedAt, picker, accept, reject);
      requests.appendChild(row);
    });
    if (!requests.childElementCount) {
      const empty = document.createElement("p");
      empty.className = "accountPanelStatus";
      empty.textContent = "No pending account requests";
      requests.appendChild(empty);
    }

    usersTitle.textContent = "Existing Accounts";
    users.className = "accountList";
    (accounts.users || []).forEach((user) => {
      const row = document.createElement("div");
      const name = document.createElement("p");
      const meta = document.createElement("p");
      const picker = renderPermissionPicker(
        user.permission_overrides?.length
          ? user.permission_overrides
          : legacyPermissionsToDecisions(user.permissions || []),
        user.groups || [],
        accounts.groups || [],
      );
      const save = document.createElement("button");
      row.className = "accountCard";
      name.className = "accountName";
      name.textContent = user.username;
      meta.className = "accountMeta";
      meta.textContent = `effective: ${(user.effective_permissions || []).join(", ") || "no permissions"}${
        user.password_required ? " - password needed" : ""
      }${user.disabled ? " - disabled" : ""}`;
      save.type = "button";
      save.textContent = "Save";
      save.addEventListener("click", async () => {
        save.disabled = true;
        try {
          await app.authRequest("/auth/admin/update-user-permissions", {
            username: user.username,
            permission_overrides: selectedPermissionDecisions(picker),
            groups: selectedGroups(picker),
          });
          await refreshAccountAdministration();
        } catch (error) {
          meta.textContent = error.message;
        } finally {
          save.disabled = false;
        }
      });
      row.append(name, meta, picker, save);
      users.appendChild(row);
    });

    panel.append(
      title,
      renderPermissionModelEditor(accounts),
      createCard,
      requestsTitle,
      requests,
      usersTitle,
      users,
    );
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
      <section class="accountAdminPanel"></section>
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
    renderAccountAdministration(root.querySelector(".accountAdminPanel"));
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
