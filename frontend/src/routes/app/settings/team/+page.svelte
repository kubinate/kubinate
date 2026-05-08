<script lang="ts">
  import { onMount } from 'svelte';
  import {
    ApiError,
    createInvite,
    listInvites,
    listMembers,
    removeMember,
    revokeInvite,
    updateMemberRole
  } from '$lib/api/team';
  import { getMe } from '$lib/api/me';
  import type { InviteView, MembershipRole, MembershipView } from '$lib/api/schemas';

  // `null` => not loaded yet, `[]` => loaded but empty.
  let members = $state<MembershipView[] | null>(null);
  let invites = $state<InviteView[] | null>(null);
  let organizationId = $state<string | null>(null);
  let loadError = $state<string | null>(null);

  // Invite form fields.
  let inviteEmail = $state('');
  let inviteRole = $state<MembershipRole>('developer');
  let inviteFormError = $state<string | null>(null);
  let inviteSubmitting = $state(false);

  // The token returned by the server is shown once and then forgotten.
  let lastIssuedToken = $state<string | null>(null);

  // Tracks per-row "in flight" so a slow PATCH can disable that row's
  // controls without freezing the whole page.
  let busy = $state<Set<string>>(new Set());

  function setBusy(key: string, value: boolean) {
    const next = new Set(busy);
    if (value) next.add(key);
    else next.delete(key);
    busy = next;
  }

  function describe(err: unknown): string {
    if (err instanceof ApiError) return `${err.title}: ${err.detail}`;
    if (err instanceof Error) return err.message;
    return 'Unknown error';
  }

  async function refresh() {
    if (!organizationId) return;
    try {
      const [m, i] = await Promise.all([listMembers(organizationId), listInvites(organizationId)]);
      members = m;
      invites = i;
    } catch (err) {
      loadError = describe(err);
    }
  }

  onMount(async () => {
    try {
      const me = await getMe();
      organizationId = me.organization_id;
      await refresh();
    } catch (err) {
      loadError = describe(err);
    }
  });

  async function onCreateInvite(event: SubmitEvent) {
    event.preventDefault();
    inviteFormError = null;
    if (!organizationId) return;
    if (!inviteEmail.includes('@')) {
      inviteFormError = 'Email looks invalid';
      return;
    }
    inviteSubmitting = true;
    try {
      const result = await createInvite(organizationId, inviteEmail, inviteRole);
      lastIssuedToken = result.token;
      inviteEmail = '';
      await refresh();
    } catch (err) {
      inviteFormError = describe(err);
    } finally {
      inviteSubmitting = false;
    }
  }

  async function onRoleChange(member: MembershipView, role: MembershipRole) {
    if (!organizationId) return;
    setBusy(`role-${member.user_id}`, true);
    try {
      await updateMemberRole(organizationId, member.user_id, role);
      await refresh();
    } catch (err) {
      loadError = describe(err);
    } finally {
      setBusy(`role-${member.user_id}`, false);
    }
  }

  async function onRemoveMember(member: MembershipView) {
    if (!organizationId) return;
    setBusy(`remove-${member.user_id}`, true);
    try {
      await removeMember(organizationId, member.user_id);
      await refresh();
    } catch (err) {
      loadError = describe(err);
    } finally {
      setBusy(`remove-${member.user_id}`, false);
    }
  }

  async function onRevokeInvite(invite: InviteView) {
    if (!organizationId) return;
    setBusy(`invite-${invite.id}`, true);
    try {
      await revokeInvite(organizationId, invite.id);
      await refresh();
    } catch (err) {
      loadError = describe(err);
    } finally {
      setBusy(`invite-${invite.id}`, false);
    }
  }

  function inviteStatus(invite: InviteView): string {
    if (invite.accepted_at) return 'accepted';
    if (invite.revoked_at) return 'revoked';
    if (new Date(invite.expires_at) < new Date()) return 'expired';
    return 'pending';
  }
</script>

<svelte:head>
  <title>Team — Kubinate</title>
</svelte:head>

<h1>Team</h1>

{#if loadError}
  <p class="error" role="alert">{loadError}</p>
{/if}

<section class="invite">
  <h2>Invite a teammate</h2>
  <form onsubmit={onCreateInvite}>
    <label>
      Email
      <input
        type="email"
        bind:value={inviteEmail}
        autocomplete="off"
        placeholder="teammate@example.com"
        data-testid="invite-email"
        required
      />
    </label>
    <label>
      Role
      <select bind:value={inviteRole} data-testid="invite-role">
        <option value="admin">Admin</option>
        <option value="developer">Developer</option>
        <option value="viewer">Viewer</option>
      </select>
    </label>
    <button type="submit" disabled={inviteSubmitting} data-testid="invite-submit">
      {inviteSubmitting ? 'Issuing…' : 'Send invite'}
    </button>
  </form>
  {#if inviteFormError}
    <p class="error" role="alert">{inviteFormError}</p>
  {/if}

  {#if lastIssuedToken}
    <div class="token-callout" role="alert" data-testid="issued-token">
      <p>Share this invite token with the recipient. We won't show it again.</p>
      <code>{lastIssuedToken}</code>
    </div>
  {/if}
</section>

<section class="members">
  <h2>Members</h2>
  {#if members === null}
    <p class="muted">Loading…</p>
  {:else if members.length === 0}
    <p class="muted">No members yet.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Name</th>
          <th>Email</th>
          <th>Role</th>
          <th>Joined</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each members as member (member.user_id)}
          <tr data-testid={`member-row-${member.user_id}`}>
            <td>{member.display_name}</td>
            <td>{member.email}</td>
            <td>
              <select
                value={member.role}
                onchange={(e) =>
                  onRoleChange(
                    member,
                    (e.currentTarget as HTMLSelectElement).value as MembershipRole
                  )}
                disabled={busy.has(`role-${member.user_id}`)}
                data-testid={`role-select-${member.user_id}`}
              >
                <option value="owner">Owner</option>
                <option value="admin">Admin</option>
                <option value="developer">Developer</option>
                <option value="viewer">Viewer</option>
              </select>
            </td>
            <td>{new Date(member.joined_at).toLocaleDateString()}</td>
            <td>
              <button
                type="button"
                onclick={() => onRemoveMember(member)}
                disabled={busy.has(`remove-${member.user_id}`)}
                data-testid={`remove-${member.user_id}`}
              >
                Remove
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<section class="invites">
  <h2>Invites</h2>
  {#if invites === null}
    <p class="muted">Loading…</p>
  {:else if invites.length === 0}
    <p class="muted">No invites issued yet.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Email</th>
          <th>Role</th>
          <th>Status</th>
          <th>Expires</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each invites as invite (invite.id)}
          <tr>
            <td>{invite.email}</td>
            <td>{invite.role}</td>
            <td>{inviteStatus(invite)}</td>
            <td>{new Date(invite.expires_at).toLocaleString()}</td>
            <td>
              {#if inviteStatus(invite) === 'pending'}
                <button
                  type="button"
                  onclick={() => onRevokeInvite(invite)}
                  disabled={busy.has(`invite-${invite.id}`)}
                  data-testid={`revoke-${invite.id}`}
                >
                  Revoke
                </button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  h1 {
    font-size: 1.8rem;
    margin-bottom: 1rem;
  }
  h2 {
    font-size: 1.1rem;
    margin: 1.5rem 0 0.5rem;
  }
  section {
    max-width: 720px;
  }
  form {
    display: grid;
    grid-template-columns: 1fr max-content max-content;
    gap: 0.75rem;
    align-items: end;
  }
  label {
    display: grid;
    gap: 0.25rem;
    font-weight: 600;
  }
  input,
  select {
    padding: 0.4rem;
    font: inherit;
    border: 1px solid #bbb;
    border-radius: 4px;
    font-weight: 400;
  }
  button {
    padding: 0.5rem 1rem;
    background: #222;
    color: #fff;
    border: 0;
    border-radius: 4px;
    font-weight: 600;
    cursor: pointer;
  }
  button[disabled] {
    opacity: 0.6;
    cursor: not-allowed;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    margin-top: 0.5rem;
  }
  th,
  td {
    padding: 0.4rem 0.5rem;
    text-align: left;
    border-bottom: 1px solid #eee;
  }
  th {
    font-size: 0.85rem;
    color: #555;
    font-weight: 600;
  }
  .muted {
    color: #555;
  }
  .error {
    color: #b00020;
  }
  .token-callout {
    margin-top: 1rem;
    padding: 0.75rem 1rem;
    background: #fff8e1;
    border: 1px solid #f0c95a;
    border-radius: 6px;
  }
  .token-callout code {
    display: block;
    margin-top: 0.5rem;
    padding: 0.5rem;
    background: #fff;
    border: 1px solid #ddd;
    border-radius: 4px;
    font-family: ui-monospace, Menlo, monospace;
    word-break: break-all;
  }
</style>
