shader_type canvas_item;

// Draws a colored outline around the sprite's opaque pixels.
uniform vec4 outline_color : source_color = vec4(1.0, 1.0, 1.0, 1.0);
uniform float outline_width : hint_range(0.0, 8.0) = 1.0;

void fragment() {
	vec4 col = texture(TEXTURE, UV);
	vec2 size = TEXTURE_PIXEL_SIZE * outline_width;

	float alpha = col.a;
	alpha = max(alpha, texture(TEXTURE, UV + vec2(0.0, -size.y)).a);
	alpha = max(alpha, texture(TEXTURE, UV + vec2(0.0, size.y)).a);
	alpha = max(alpha, texture(TEXTURE, UV + vec2(-size.x, 0.0)).a);
	alpha = max(alpha, texture(TEXTURE, UV + vec2(size.x, 0.0)).a);

	vec4 final = mix(outline_color, col, col.a);
	COLOR = vec4(final.rgb, alpha);
}
