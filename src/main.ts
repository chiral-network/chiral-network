import App from './App.svelte'
import { mount } from 'svelte';
import './styles/globals.css'

console.log('Main.ts loading...')

const target = document.getElementById('app')
console.log('Target element:', target)

let app: any = null

if (!target) {
  console.error('Could not find app element!')
  document.body.innerHTML = '<h1 style="color: red;">Error: Could not find app element!</h1>'
} else {
  try {
    app = mount (App, {
      target: target,
    })
    console.log('App created successfully')
  } catch (error) {
    console.error('Error creating app:', error)
    document.body.innerHTML = `<h1 style="color: red;">Error: ${error}</h1>`
  }
}

export default app