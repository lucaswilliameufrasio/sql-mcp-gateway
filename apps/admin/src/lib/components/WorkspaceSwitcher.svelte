<script lang="ts">
  import Icon from './Icon.svelte';
  import {
    environmentLabel,
    showsEnvironmentTag,
    switchUrl,
    type WorkspaceDirectory
  } from '$lib/workspaces';

  let { directory, pathname }: { directory: WorkspaceDirectory; pathname: string } = $props();
</script>

{#snippet identity(workspace: WorkspaceDirectory['current'])}
  <span class="workspace-identity">
    <strong>{workspace.name}</strong>
    {#if showsEnvironmentTag(workspace)}
      <span class="environment-tag environment-{workspace.environment}"
        >{environmentLabel(workspace.environment)}</span
      >
    {/if}
  </span>
{/snippet}

{#if directory.others.length === 0}
  <div class="workspace-switcher workspace-single" aria-label="Workspace atual">
    {@render identity(directory.current)}
  </div>
{:else}
  <details class="workspace-switcher">
    <summary aria-label="Trocar de workspace. Atual: {directory.current.name}">
      {@render identity(directory.current)}
      <Icon name="chevron" size={15} />
    </summary>
    <ul aria-label="Outros workspaces">
      {#each directory.others as workspace (workspace.slug)}
        <li>
          <a href={switchUrl(workspace, pathname)} rel="external">
            {@render identity(workspace)}
            <span class="workspace-host">{new URL(workspace.origin).host}</span>
          </a>
        </li>
      {/each}
    </ul>
  </details>
{/if}
