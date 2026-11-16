<script lang="ts">
  import { pillTone, type IconComponent, type Tone } from './tones';
  interface Props { label: string; value: string | number; icon: IconComponent; tone?: Tone; mono?: boolean; title?: string; spin?: boolean; valueClass?: string }
  let { label, value, icon: Icon, tone = 'primary', mono = false, title = undefined, spin = false, valueClass = '' }: Props = $props();
</script>

<div class="flex items-center gap-3 px-3 py-2.5 rounded-xl bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 min-w-0" {title}>
  <div class="p-2 rounded-lg shrink-0 {pillTone[tone]}">
    <Icon class="w-4 h-4 {spin ? 'animate-spin' : ''}" />
  </div>
  <div class="min-w-0">
    <div class="stat-label truncate">{label}</div>
    <div class="stat-value truncate {mono ? 'font-mono text-xs' : ''} {valueClass}">{value}</div>
  </div>
</div>
