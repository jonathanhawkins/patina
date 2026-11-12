shader_type spatial;
render_mode unshaded;

uniform vec4 stripe_color : source_color = vec4(1.0, 0.5, 0.0, 1.0);
uniform float stripes = 4.0;

float band(float x) {
	return step(0.5, fract(x));
}

void fragment() {
	float b = band(UV.x * stripes);
	vec3 base = vec3(0.1, 0.3, 0.9);
	vec3 c = mix(base, stripe_color.rgb, b);
	if (UV.y > 0.5) {
		c *= 0.5;
	}
	ALBEDO = c;
}
