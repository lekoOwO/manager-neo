import { createRouter, createWebHistory } from 'vue-router'

import InstancesView from '@/views/InstancesView.vue'
import ModelsView from '@/views/ModelsView.vue'
import TemplatesView from '@/views/TemplatesView.vue'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', component: InstancesView },
    { path: '/models', component: ModelsView },
    { path: '/templates', component: TemplatesView },
  ],
})

export default router
