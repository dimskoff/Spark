-- Immediate-mode UI: a slider, a checkbox and buttons drive a spinning cube.
local cube
local speed, wire, count = 1.0, true, 0

function start()
    sun(vec3(-0.4, -1, -0.3), "white", 1.2)
    spawn(Mesh.plane(20), "#445566")
    cube = spawn(Mesh.cube(), "orange", { position = vec3(0, 0.5, 0) })
    camera.position = vec3(3, 2.5, 4)
    camera:look_at(vec3(0, 0.5, 0))
end

function update(dt)
    cube:rotate_y(dt * speed)
end

function render()
    ui.panel(16, 16, 260, 170)
    ui.label(28, 24, "Speed: " .. string.format("%.1f", speed), 20)
    speed = ui.slider("speed", 28, 50, 230, 24, speed, 0, 5)
    wire = ui.checkbox("shadow", 28, 86, 24, wire)
    ui.label(60, 88, "Cast shadow", 20)
    cube.cast_shadow = wire
    if ui.button("add", 28, 126, 110, 36, "Add cube") then
        count += 1
        spawn(Mesh.cube(0.4), "#66ccff", { position = vec3(-2 + count * 0.6, 0.2, 1.5) })
    end
    if ui.button("clear", 148, 126, 110, 36, "Reset") then speed, count = 1, 0 end
end