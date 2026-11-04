shader_type canvas_item;

uniform vec4 top_color : source_color = vec4(0.07, 0.05, 0.16, 1.0);
uniform vec4 bottom_color : source_color = vec4(0.02, 0.02, 0.06, 1.0);

void fragment() {
	vec4 c = mix(top_color, bottom_color, smoothstep(0.0, 1.0, UV.y));
	c.rgb *= 1.0 - distance(UV, vec2(0.5)) * 0.7;
	c.rgb += 0.02 * sin(TIME * 0.7 + UV.x * 6.0);
	COLOR = c;
}
