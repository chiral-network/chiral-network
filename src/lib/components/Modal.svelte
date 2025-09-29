<script lang="ts">
	import { createEventDispatcher } from 'svelte';
	import { X } from 'lucide-svelte';

	export let showModal: boolean;

	const dispatch = createEventDispatcher();

	function closeModal() {
		dispatch('close');
	}
</script>

{#if showModal}
	<div
		class="fixed inset-0 bg-black bg-opacity-60 z-40 flex justify-center items-center"
		on:click={closeModal}
		on:keydown
	>
		<div class="bg-gray-800 rounded-lg shadow-xl z-50 w-full max-w-2xl" on:click|stopPropagation>
			<button on:click={closeModal} class="absolute top-3 right-3 text-gray-400 hover:text-white">
				<X />
			</button>
			<slot />
		</div>
	</div>
{/if}