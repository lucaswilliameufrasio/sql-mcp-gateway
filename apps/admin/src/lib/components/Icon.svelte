<script lang="ts">
  export type IconName =
    | 'database'
    | 'home'
    | 'plug'
    | 'users'
    | 'audit'
    | 'logout'
    | 'plus'
    | 'arrow'
    | 'check'
    | 'search'
    | 'shield'
    | 'copy'
    | 'refresh'
    | 'chevron';
  let { name, size = 20 }: { name: IconName; size?: number } = $props();
  const paths: Record<IconName, string> = {
    database:
      'M20 6c0 2.2-16 2.2-16 0s16-2.2 16 0ZM4 6v12c0 2.2 16 2.2 16 0V6M4 12c0 2.2 16 2.2 16 0',
    home: 'm3 10 9-7 9 7M5 9v12h14V9M9 21v-8h6v8',
    plug: 'm8 3 0 5m8-5v5M6 8h12v3a6 6 0 0 1-12 0V8Zm6 9v4',
    users:
      'M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M16 3a4 4 0 0 1 0 8m6 10v-2a4 4 0 0 0-3-3.9M13 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z',
    audit: 'M9 5H5v16h14V5h-4M9 3h6v4H9V3Zm-1 9h8m-8 4h6',
    logout: 'M9 4H4v16h5m5-12 4 4-4 4m-6-4h13',
    plus: 'M12 5v14M5 12h14',
    arrow: 'M4 12h16m-6-6 6 6-6 6',
    check: 'm5 12 4 4L19 6',
    search: 'm21 21-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z',
    shield: 'm12 3 8 3v6c0 5-8 9-8 9s-8-4-8-9V6l8-3Zm-4 9 3 3 5-5',
    copy: 'M9 9h12v12H9V9ZM15 9V3H3v12h6',
    refresh: 'M20 7v5h-5M4 17v-5h5M5 7a8 8 0 0 1 13-2l2 3M4 16l2 3a8 8 0 0 0 13-2',
    chevron: 'm9 5 7 7-7 7'
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"><path d={paths[name]} /></svg
>
