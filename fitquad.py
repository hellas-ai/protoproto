import numpy as np
import matplotlib.pyplot as plt

# Define the data points
x = np.array([16, 32, 64, 128])
y = np.array([38.6, 77.42, 149.93, 297])

# Fit a quadratic polynomial (degree=2)
coeffs = np.polyfit(x, y, 2)

# Create a polynomial function from the coefficients
quadratic = np.poly1d(coeffs)

# Print the fitted polynomial
print("Fitted quadratic polynomial:")
print(quadratic)

# Generate x values for plotting the fitted curve
x_fit = np.linspace(x.min(), x.max(), 100)
y_fit = quadratic(x_fit)

# Plot the original data points and the fitted quadratic curve
plt.scatter(x, y, color='blue', label='Data Points')
plt.plot(x_fit, y_fit, color='red', label='Quadratic Fit')
plt.xlabel('x')
plt.ylabel('y')
plt.title('Quadratic Fit to Data')
plt.legend()
plt.show()
