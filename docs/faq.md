---
layout: page
title: Questions and answers
description: "Killing whatever is on a port, fixing “address already in use”, finding zombie and orphan processes, and more, on macOS, Linux, Windows, FreeBSD and NetBSD."
permalink: /faq
faq: true
---

<nav class="faq-index" aria-label="Questions">
  <ul>
  {%- for item in site.data.faq %}
    <li><a href="#{{ item.id }}">{{ item.q | smartify }}</a></li>
  {%- endfor %}
  </ul>
</nav>

{% for item in site.data.faq %}
<section class="faq-item" id="{{ item.id }}">
  <h2>{{ item.q | smartify }}</h2>
  {{ item.a | markdownify }}
</section>
{% endfor %}
